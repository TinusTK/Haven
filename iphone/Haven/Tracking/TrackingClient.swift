import Foundation

/// Talks to Trakt and Simkl.
struct TrackingClient: Sendable {
    static let traktAPI = "https://api.trakt.tv"
    static let simklAPI = "https://api.simkl.com"
    static let answerLimit = 16 * 1024 * 1024
    /// A token is renewed when it has less than this long to live (seconds).
    static let renewBefore: Int64 = 60 * 60
    /// Trakt's redirect address registered for Haven's app (sent with token
    /// requests, as the Rust build did).
    static let traktRedirect = "http://127.0.0.1:47219/trakt"

    var keys: Secrets
    var http: HTTP = .shared
    var trakt = TrackingClient.traktAPI
    var simkl = TrackingClient.simklAPI

    init(keys: Secrets, http: HTTP = .shared) {
        self.keys = keys
        self.http = http
    }

    /// Whether the keys a service needs are set. Trakt signs in with the
    /// Client ID alone (new Trakt apps get no Client Secret).
    func hasKeys(_ service: TrackingService) -> Bool {
        service == .trakt ? keys.traktClientID != nil : keys.simklClientID != nil
    }

    private func traktKeys() throws -> (id: String, secret: String?) {
        guard let id = keys.traktClientID else { throw TrackingError.noKey("Trakt") }
        return (id, keys.traktClientSecret)
    }

    private func simklKey() throws -> String {
        guard let id = keys.simklClientID else { throw TrackingError.noKey("Simkl") }
        return id
    }

    private func headers(_ account: TrackedAccount) throws -> [(String, String)] {
        let bearer = ("Authorization", "Bearer \(account.token)")
        switch account.service {
        case .trakt: return [bearer, ("trakt-api-version", "2"), ("trakt-api-key", try traktKeys().id)]
        case .simkl: return [bearer, ("simkl-api-key", try simklKey())]
        }
    }

    /// Adds the Client Secret to a Trakt token request, when there is one.
    static func withSecret(_ body: [String: Any], _ secret: String?) -> [String: Any] {
        guard let secret else { return body }
        var body = body
        body["client_secret"] = secret
        return body
    }

    /// Percent-encodes a form or query value.
    static func formEscape(_ text: String) -> String { ResourceURL.encoded(text) }

    /// A Simkl address with what Simkl asks every request to carry: the
    /// app's client id, name and version.
    static func simklURL(_ base: String, _ path: String, clientID: String) -> String {
        let joiner = path.contains("?") ? "&" : "?"
        return "\(base)\(path)\(joiner)client_id=\(formEscape(clientID))&app-name=haven&app-version=\(Build.version)"
    }

    /// A JSON request; an answer that is not JSON reads as nil.
    private func call(_ service: TrackingService, _ url: String, headers: [(String, String)] = [],
                      body: [String: Any]? = nil) async throws -> (status: Int, answer: Any?) {
        let answer: HTTPAnswer
        do {
            answer = try await http.send(url, headers: headers, body: try body.map { try JSONSerialization.data(withJSONObject: $0) },
                                         limit: Self.answerLimit)
        } catch let error as HTTPError {
            throw TrackingError.http(error)
        }
        if answer.status == 401 || answer.status == 403 { throw TrackingError.refused(service.label) }
        return (answer.status, try? JSONSerialization.jsonObject(with: answer.body, options: [.fragmentsAllowed]))
    }

    /// A form-encoded POST to Simkl's sign-in addresses (AUTH V2).
    private func simklForm(_ path: String, _ fields: [(String, String)]) async throws -> (status: Int, answer: Any?) {
        let body = fields.map { "\(Self.formEscape($0.0))=\(Self.formEscape($0.1))" }.joined(separator: "&")
        let answer: HTTPAnswer
        do {
            answer = try await http.send(Self.simklURL(simkl, path, clientID: try simklKey()), body: Data(body.utf8),
                                         contentType: "application/x-www-form-urlencoded", limit: Self.answerLimit)
        } catch HTTPError.tooLarge {
            throw TrackingError.badAnswer
        } catch let error as HTTPError {
            throw TrackingError.network(error.localizedDescription)
        }
        return (answer.status, try? JSONSerialization.jsonObject(with: answer.body, options: [.fragmentsAllowed]))
    }

    // MARK: Signing in

    /// Starts a sign-in and gives the code to show the user.
    func begin(_ service: TrackingService) async throws -> SignInCode {
        func field(_ answer: Any?, _ key: String) throws -> String {
            guard let text = (answer as? [String: Any])?[key] as? String else { throw TrackingError.badAnswer }
            return text
        }
        func seconds(_ answer: Any?, _ key: String, _ fallback: Int) -> Int {
            TMDB.whole((answer as? [String: Any])?[key]).map { Int($0) } ?? fallback
        }
        switch service {
        case .trakt:
            let (status, answer) = try await call(service, "\(trakt)/oauth/device/code", body: ["client_id": try traktKeys().id])
            guard status == 200 else { throw TrackingError.status(status) }
            return SignInCode(service: service, userCode: try field(answer, "user_code"), link: try field(answer, "verification_url"),
                              deviceCode: try field(answer, "device_code"), interval: max(seconds(answer, "interval", 5), 1),
                              expiresIn: seconds(answer, "expires_in", 600))
        case .simkl:
            // The person approves the code at simkl.com/pin (the link has it filled in).
            let (status, answer) = try await simklForm("/oauth2/device", [("client_id", try simklKey()), ("scope", "media:read media:write")])
            guard status == 200 else { throw TrackingError.status(status) }
            let userCode = try field(answer, "user_code")
            let link = try (try? field(answer, "verification_uri_complete")) ?? field(answer, "verification_uri")
            return SignInCode(service: service, userCode: userCode, link: link, deviceCode: try field(answer, "device_code"),
                              interval: max(seconds(answer, "interval", 5), 1), expiresIn: seconds(answer, "expires_in", 900))
        }
    }

    /// Asks whether the user has approved the code yet; nil while not.
    func poll(_ code: SignInCode, now: Int64) async throws -> TrackedAccount? {
        switch code.service {
        case .trakt:
            let (id, secret) = try traktKeys()
            let (status, answer) = try await call(.trakt, "\(trakt)/oauth/device/token",
                                                  body: Self.withSecret(["code": code.deviceCode, "client_id": id], secret))
            switch status {
            case 200:
                guard let account = Tracking.readTraktToken(answer, now: now) else { throw TrackingError.badAnswer }
                return account
            case 400, 429: return nil // not approved yet, or asked too soon
            case 404, 409, 410, 418: throw TrackingError.expired
            default: throw TrackingError.status(status)
            }
        case .simkl:
            let (status, answer) = try await simklForm("/oauth2/token", [
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", try simklKey()), ("device_code", code.deviceCode),
            ])
            if status == 200 {
                guard let account = Tracking.readSimklToken(answer, now: now, refresh: nil) else { throw TrackingError.badAnswer }
                return account
            }
            switch (answer as? [String: Any])?["error"] as? String {
            case "authorization_pending", "slow_down": return nil // not approved yet (a "no" looks the same)
            case "expired_token", "invalid_grant": throw TrackingError.expired
            default: throw TrackingError.status(status)
            }
        }
    }

    /// A renewed account if this one's token is about to run out; nil if it
    /// is still good (or never runs out).
    func renewed(_ account: TrackedAccount, now: Int64) async throws -> TrackedAccount? {
        guard let expiresAt = account.expiresAt, let refresh = account.refresh, expiresAt - now <= Self.renewBefore else { return nil }
        if account.service == .simkl {
            let (status, answer) = try await simklForm("/oauth2/token", [
                ("grant_type", "refresh_token"), ("client_id", try simklKey()), ("refresh_token", refresh),
            ])
            guard status == 200 else { throw TrackingError.refused("Simkl") }
            // Simkl keeps the same refresh token (it may not send it again).
            guard let renewed = Tracking.readSimklToken(answer, now: now, refresh: refresh) else { throw TrackingError.badAnswer }
            return renewed
        }
        let (id, secret) = try traktKeys()
        let (status, answer) = try await call(.trakt, "\(trakt)/oauth/token", body: Self.withSecret([
            "refresh_token": refresh, "client_id": id, "redirect_uri": Self.traktRedirect, "grant_type": "refresh_token",
        ], secret))
        guard status == 200 else { throw TrackingError.refused("Trakt") }
        guard let renewed = Tracking.readTraktToken(answer, now: now) else { throw TrackingError.badAnswer }
        return renewed
    }

    /// Tells the service to forget the sign-in (best effort).
    func revoke(_ account: TrackedAccount) async throws {
        switch account.service {
        case .trakt:
            let (id, secret) = try traktKeys()
            _ = try await call(.trakt, "\(trakt)/oauth/revoke", body: Self.withSecret(["token": account.token, "client_id": id], secret))
        case .simkl:
            _ = try await simklForm("/oauth2/revoke", [("client_id", try simklKey()), ("token", account.token)])
        }
    }

    // MARK: Keeping in step

    private func post(_ account: TrackedAccount, _ path: String, _ body: [String: Any]) async throws {
        let url = account.service == .trakt ? "\(trakt)\(path)" : Self.simklURL(simkl, path, clientID: try simklKey())
        let (status, _) = try await call(account.service, url, headers: try headers(account), body: body)
        guard (200 ..< 300).contains(status) else { throw TrackingError.status(status) }
    }

    /// Marks a film or an episode as watched, now.
    func markWatched(_ account: TrackedAccount, _ subject: Subject) async throws {
        try await post(account, "/sync/history", Tracking.body(subject))
    }

    /// Takes a film or episode off the account's watched history.
    func markUnwatched(_ account: TrackedAccount, _ subject: Subject) async throws {
        try await post(account, "/sync/history/remove", Tracking.body(subject))
    }

    /// Puts a film or series on the service's Watchlist, or takes it off.
    /// Simkl has no way to take a title off its list without also erasing
    /// its watched history, so there a removal is not passed on.
    func setWatchlist(_ account: TrackedAccount, _ subject: Subject, on: Bool) async throws {
        var whole = subject
        whole.episode = nil
        switch (account.service, on) {
        case (.trakt, true): try await post(account, "/sync/watchlist", Tracking.body(whole))
        case (.trakt, false): try await post(account, "/sync/watchlist/remove", Tracking.body(whole))
        case (.simkl, true): try await post(account, "/sync/add-to-list", Tracking.body(whole, simklList: "plantowatch"))
        case (.simkl, false): return
        }
    }

    /// What the account has watched: films, and each episode of a series.
    /// Simkl: completed films, and the watched episodes of every series it
    /// has (whatever its list), with when each was watched.
    func watched(_ account: TrackedAccount) async throws -> [Watched] {
        let headers = try headers(account)
        var found: [Watched] = []
        for series in [false, true] {
            let url: String
            switch (account.service, series) {
            case (.trakt, _): url = "\(trakt)/sync/watched/\(series ? "shows" : "movies")"
            case (.simkl, true):
                url = Self.simklURL(simkl, "/sync/all-items/shows?extended=full&episode_watched_at=yes", clientID: try simklKey())
            case (.simkl, false): url = Self.simklURL(simkl, "/sync/all-items/movies/completed", clientID: try simklKey())
            }
            let (status, answer) = try await call(account.service, url, headers: headers)
            guard (200 ..< 300).contains(status) else { throw TrackingError.status(status) }
            found += Tracking.readWatched(answer, series: series)
        }
        return found
    }

    /// The films and series on the service's Watchlist.
    func watchlist(_ account: TrackedAccount) async throws -> [Listed] {
        let headers = try headers(account)
        var listed: [Listed] = []
        for series in [false, true] {
            let what = series ? "shows" : "movies"
            let url = account.service == .trakt
                ? "\(trakt)/sync/watchlist/\(what)"
                : Self.simklURL(simkl, "/sync/all-items/\(what)/plantowatch", clientID: try simklKey())
            let (status, answer) = try await call(account.service, url, headers: headers)
            guard (200 ..< 300).contains(status) else { throw TrackingError.status(status) }
            listed += Tracking.readListed(answer, series: series)
        }
        return listed
    }
}
