import Foundation

// Tracking what was watched with Trakt and Simkl: signing in with a short
// code, marking films and episodes watched, and keeping the Watchlist in
// step. Both services know titles by their IMDb id, so only titles that
// carry one can be tracked. Each service needs Haven's app key (a client
// id; older Trakt apps also a client secret).

enum TrackingError: Error, Equatable, LocalizedError {
    case http(HTTPError)
    case noKey(String)
    case refused(String)
    case status(Int)
    case badAnswer
    case expired
    case network(String)

    var errorDescription: String? {
        switch self {
        case let .http(error): error.errorDescription
        case let .noKey(service): "no \(service) key is set in Settings"
        case let .refused(service): "\(service) did not accept the key or the sign-in"
        case let .status(code): "the service answered with error \(code)"
        case .badAnswer: "the service's answer could not be read"
        case .expired: "the code ran out before it was approved"
        case let .network(why): "could not reach the service: \(why)"
        }
    }
}

enum TrackingService: String, Codable, Sendable, CaseIterable {
    case trakt
    case simkl

    var label: String { self == .trakt ? "Trakt" : "Simkl" }
}

/// A signed-in account, as kept between runs (kv `tracking_accounts`, the
/// Rust build's shape). Never prints its tokens.
struct TrackedAccount: Codable, Equatable, Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    static let key = "tracking_accounts"

    var service: TrackingService
    var token: String
    /// Exchanges for a new token when this one runs out.
    var refresh: String?
    /// When the token runs out, in seconds since 1970, if it does.
    var expiresAt: Int64?

    enum CodingKeys: String, CodingKey {
        case service, token, refresh
        case expiresAt = "expires_at"
    }

    var description: String { "TrackedAccount(service: \(service.rawValue), ..)" }
    var debugDescription: String { description }

    static func load(_ store: Store) -> [TrackedAccount] {
        (try? store.get(key, as: [TrackedAccount].self)) ?? []
    }

    static func save(_ accounts: [TrackedAccount], store: Store) {
        do {
            try store.put(key, accounts)
        } catch {
            Log.note("could not save the tracking sign-ins: \(error.localizedDescription)")
        }
    }
}

/// A sign-in in progress: the user approves `userCode` at `link`.
struct SignInCode: Equatable, Sendable {
    var service: TrackingService
    var userCode: String
    var link: String
    /// What Haven asks with while waiting.
    var deviceCode: String
    /// Seconds between asks.
    var interval: Int
    /// Seconds until the code runs out.
    var expiresIn: Int
}

/// A film, a series, or one episode, by IMDb id.
struct Subject: Equatable, Sendable {
    var imdb: String
    var series: Bool
    /// Season and episode numbers, for one episode.
    var episode: (season: Int, number: Int)?

    static func == (a: Subject, b: Subject) -> Bool {
        a.imdb == b.imdb && a.series == b.series && a.episode?.season == b.episode?.season
            && a.episode?.number == b.episode?.number
    }
}

/// A title on a service's Watchlist.
struct Listed: Equatable, Sendable {
    var imdb: String
    var series: Bool
    var title: String
    var year: String
}

/// A film or episode the service says was watched.
struct Watched: Equatable, Sendable {
    var imdb: String
    var title: String
    /// Season and episode for an episode; nil for a film.
    var episode: (season: Int, number: Int)?
    /// When, in seconds since 1970 (0 if not given).
    var at: Int64

    static func == (a: Watched, b: Watched) -> Bool {
        a.imdb == b.imdb && a.title == b.title && a.at == b.at
            && a.episode?.season == b.episode?.season && a.episode?.number == b.episode?.number
    }
}

enum Tracking {
    /// What to track for one of Haven's titles: nil if it has no IMDb id.
    /// `videoID` is the film's or episode's own id; an episode must be named
    /// `<imdb id>:<season>:<episode>`.
    static func subject(kind: String, titleID: String, videoID: String?) -> Subject? {
        guard TMDB.isIMDbID(titleID) else { return nil }
        let series = kind == "series"
        var episode: (Int, Int)?
        if series, let video = videoID {
            let parts = video.split(separator: ":", omittingEmptySubsequences: false)
            if parts.count == 3, parts[0] == titleID, let s = UInt32(parts[1]), let e = UInt32(parts[2]) {
                episode = (Int(s), Int(e))
            }
        }
        return Subject(imdb: titleID, series: series, episode: episode)
    }

    /// The JSON both services take to name a subject.
    static func body(_ subject: Subject, simklList: String? = nil) -> [String: Any] {
        var entry: [String: Any] = ["ids": ["imdb": subject.imdb]]
        if let simklList { entry["to"] = simklList }
        if let (season, number) = subject.episode {
            entry["seasons"] = [["number": season, "episodes": [["number": number]]]]
        }
        return subject.series ? ["shows": [entry]] : ["movies": [entry]]
    }

    /// An RFC 3339 moment in seconds since 1970; 0 if not readable.
    static func moment(_ value: Any?) -> Int64 {
        guard let text = value as? String else { return 0 }
        let withFraction = ISO8601DateFormatter()
        withFraction.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let plain = ISO8601DateFormatter()
        guard let date = withFraction.date(from: text) ?? plain.date(from: text) else { return 0 }
        return Int64(date.timeIntervalSince1970.rounded(.down))
    }

    private static func imdb(_ title: Any?) -> String? {
        guard let id = ((title as? [String: Any])?["ids"] as? [String: Any])?["imdb"] as? String, TMDB.isIMDbID(id) else { return nil }
        return id
    }

    /// Trakt's `/sync/watched/movies` or `/sync/watched/shows` answer.
    /// Trakt's `/sync/watched/…` answer, or Simkl's `/sync/all-items/…`
    /// one (wrapped in {"movies": […]} or {"shows": […]}, an episode's time
    /// in `watched_at`).
    static func readWatched(_ answer: Any?, series: Bool) -> [Watched] {
        let key = series ? "show" : "movie"
        var found: [Watched] = []
        let entries = answer as? [Any] ?? (answer as? [String: Any])?[series ? "shows" : "movies"] as? [Any] ?? []
        for entry in entries {
            guard let entry = entry as? [String: Any], let title = entry[key], let imdb = imdb(title) else { continue }
            let name = (title as? [String: Any])?["title"] as? String ?? ""
            if !series {
                found.append(Watched(imdb: imdb, title: name, episode: nil, at: moment(entry["last_watched_at"])))
                continue
            }
            for season in entry["seasons"] as? [Any] ?? [] {
                guard let number = TMDB.whole((season as? [String: Any])?["number"]) else { continue }
                for episode in (season as? [String: Any])?["episodes"] as? [Any] ?? [] {
                    guard let episodeNumber = TMDB.whole((episode as? [String: Any])?["number"]) else { continue }
                    found.append(Watched(imdb: imdb, title: name, episode: (Int(number), Int(episodeNumber)),
                                         at: moment((episode as? [String: Any])?["last_watched_at"]
                                             ?? (episode as? [String: Any])?["watched_at"])))
                }
            }
        }
        return found
    }

    /// A Watchlist answer: Trakt answers with a list; Simkl wraps it in
    /// {"movies": […]} or {"shows": […]}.
    static func readListed(_ answer: Any?, series: Bool) -> [Listed] {
        let key = series ? "show" : "movie"
        let entries = answer as? [Any] ?? (answer as? [String: Any])?[series ? "shows" : "movies"] as? [Any] ?? []
        return entries.compactMap { entry in
            guard let title = (entry as? [String: Any])?[key] as? [String: Any], let imdb = imdb(title),
                  let name = title["title"] as? String else { return nil }
            let year: String
            switch title["year"] {
            case let number as NSNumber where !JSONObject.isBool(number): year = JSONObject.text(number) ?? ""
            case let text as String: year = text
            default: year = ""
            }
            return Listed(imdb: imdb, series: series, title: name, year: year)
        }
    }

    /// A Simkl sign-in from a token answer; `refresh` is kept when the answer
    /// leaves the refresh token out.
    static func readSimklToken(_ answer: Any?, now: Int64, refresh: String?) -> TrackedAccount? {
        let answer = answer as? [String: Any]
        guard let token = answer?["access_token"] as? String else { return nil }
        return TrackedAccount(service: .simkl, token: token, refresh: answer?["refresh_token"] as? String ?? refresh,
                              expiresAt: integer(answer?["expires_in"]).map { now + $0 })
    }

    static func readTraktToken(_ answer: Any?, now: Int64) -> TrackedAccount? {
        let answer = answer as? [String: Any]
        guard let token = answer?["access_token"] as? String else { return nil }
        return TrackedAccount(service: .trakt, token: token, refresh: answer?["refresh_token"] as? String,
                              expiresAt: integer(answer?["expires_in"]).map { (integer(answer?["created_at"]) ?? now) + $0 })
    }

    /// A whole number (serde's as_i64).
    private static func integer(_ value: Any?) -> Int64? {
        guard let number = value as? NSNumber, !JSONObject.isBool(number), !JSONObject.isFloat(number) else { return nil }
        return number.int64Value
    }
}
