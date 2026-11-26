#if ADDONS
import Foundation

/// A person's Stremio account (Settings › Accounts, GitHub build): signing
/// in, the list of services the account uses (each with its address and
/// manifest), and signing out. Haven then asks those services itself, as
/// Stremio does; the account only holds the list. As in the Rust build
/// (`haven-addons/src/stremio.rs`, `library/stremio.rs` on a computer):
/// add-ons are installed by hand too, so only those the account brought in
/// (kv `stremio_followed`) follow it. The account (email and Stremio's key,
/// never the password) is kept under `stremio_account`; not in backups.
///
/// Stremio's API answers every call with `{"result": …}` or
/// `{"error": {"message": …}}`.
enum Stremio {
    static let api = "https://api.strem.io/api"
    static let accountKey = "stremio_account"
    static let followedKey = "stremio_followed"
    /// The most an answer from Stremio may hold.
    static let mostBytes = 8 * 1024 * 1024

    /// A signed-in account, as the Rust build keeps it. Never log `authKey`.
    struct Account: Codable, Equatable, Sendable, CustomStringConvertible {
        var email: String
        var authKey: String

        enum CodingKeys: String, CodingKey {
            case email
            case authKey = "auth_key"
        }

        var description: String { "Stremio account \(email)" }
    }

    /// One service on the account: its address and its manifest (raw JSON).
    struct Listed: Equatable, Sendable {
        var url: String
        var manifest: String
    }

    enum Failure: Error, Equatable, LocalizedError {
        /// What Stremio said, e.g. a wrong password.
        case refused(String)
        case unreachable
        case badAnswer

        var errorDescription: String? {
            switch self {
            case let .refused(why): why
            case .unreachable: "Stremio could not be reached"
            case .badAnswer: "Stremio's answer could not be read"
            }
        }
    }

    // MARK: Calls

    /// Signs in with an email address and password.
    static func login(email: String, password: String, http: HTTP = .shared) async throws -> Account {
        let body: [String: Any] = ["type": "Login", "email": email.trimmingCharacters(in: .whitespaces),
                                   "password": password, "facebook": false]
        return try parseLogin(try await call("login", body, http: http))
    }

    /// The services on the account, in the account's order.
    static func listed(_ account: Account, http: HTTP = .shared) async throws -> [Listed] {
        try parseListed(try await call("addonCollectionGet",
                                       ["type": "AddonCollectionGet", "authKey": account.authKey, "update": true], http: http))
    }

    /// Ends the sign-in on Stremio's side (best effort).
    static func logout(_ account: Account, http: HTTP = .shared) async {
        _ = try? await call("logout", ["type": "Logout", "authKey": account.authKey], http: http)
    }

    private static func call(_ method: String, _ body: [String: Any], http: HTTP) async throws -> Any {
        let data = try JSONSerialization.data(withJSONObject: body)
        let answer: HTTPAnswer
        do {
            answer = try await http.send("\(api)/\(method)", body: data, limit: mostBytes)
        } catch {
            throw Failure.unreachable
        }
        return try result(answer.body)
    }

    /// The `result` of an answer, or what its `error` says.
    static func result(_ body: Data) throws -> Any {
        guard let answer = (try? JSONSerialization.jsonObject(with: body)) as? [String: Any] else { throw Failure.badAnswer }
        if let error = answer["error"], !(error is NSNull) {
            let message = (error as? [String: Any])?["message"] as? String
            throw Failure.refused(message ?? "Stremio said no")
        }
        guard let result = answer["result"], !(result is NSNull) else { throw Failure.badAnswer }
        return result
    }

    static func parseLogin(_ result: Any) throws -> Account {
        guard let result = result as? [String: Any],
              let key = result["authKey"] as? String, !key.isEmpty else { throw Failure.badAnswer }
        let email = (result["user"] as? [String: Any])?["email"] as? String ?? ""
        return Account(email: email, authKey: key)
    }

    static func parseListed(_ result: Any) throws -> [Listed] {
        guard let list = (result as? [String: Any])?["addons"] as? [Any] else { throw Failure.badAnswer }
        return list.compactMap { item in
            guard let entry = item as? [String: Any],
                  let url = (entry["transportUrl"] as? String)?.trimmingCharacters(in: .whitespaces),
                  let manifest = entry["manifest"] as? [String: Any],
                  wanted(url),
                  let data = try? JSONSerialization.data(withJSONObject: manifest, options: [.sortedKeys]),
                  let json = String(data: data, encoding: .utf8) else { return nil }
            return Listed(url: url, manifest: json)
        }
    }

    /// Leaves out Cinemeta (built into Haven) and services that only run on
    /// the computer Stremio itself runs on.
    static func wanted(_ url: String) -> Bool {
        guard let parsed = URL(string: url), let scheme = parsed.scheme?.lowercased(),
              scheme == "http" || scheme == "https" else { return false }
        let host = parsed.host?.lowercased() ?? ""
        let local = ["127.0.0.1", "localhost", "::1", "[::1]"].contains(host)
        return !local && url != Cinemeta.url
    }

    /// What to add and remove so the add-ons that came from the account
    /// (`followed`) match its list, leaving those installed by hand alone.
    /// Also the new `followed`: what the account brought in (one already
    /// installed by hand is not taken).
    static func followedChanges(have: [String], followed: [String], wanted: [Listed])
        -> (add: [Listed], remove: [String], kept: [String]) {
        let add = wanted.filter { !have.contains($0.url) }
        let listed = { (url: String) in wanted.contains { $0.url == url } }
        let remove = followed.filter { !listed($0) && have.contains($0) }
        let kept = followed.filter(listed) + add.map(\.url)
        return (add, remove, kept)
    }

    // MARK: Keeping it

    static func account(_ store: Store) -> Account? {
        (try? store.get(accountKey, as: Account.self)) ?? nil
    }

    /// Makes what is installed match the account's list. True when
    /// anything changed.
    @discardableResult
    static func follow(_ listed: [Listed], store: Store) -> Bool {
        let have = ((try? store.addons()) ?? []).map(\.url)
        let followed = ((try? store.get(followedKey, as: [String].self)) ?? nil) ?? []
        let (add, remove, kept) = followedChanges(have: have, followed: followed, wanted: listed)
        if kept.isEmpty { try? store.remove(followedKey) } else { try? store.put(followedKey, kept) }
        if add.isEmpty && remove.isEmpty { return false }
        for url in remove { try? InstalledAddons.remove(url, store: store) }
        for item in add { try? store.saveAddon(url: item.url, manifest: item.manifest) }
        return true
    }
}
#endif
