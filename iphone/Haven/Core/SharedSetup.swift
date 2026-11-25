import Foundation

/// What the iPhone/iPad and Apple TV apps share through iCloud (user,
/// 2026-11-25: media servers, Live TV and accounts only): media servers
/// with their sign-ins, Live TV sources and Favourites, and the Trakt and
/// Simkl sign-ins, with what is switched off. Watched marks, the
/// Watchlist and other settings stay on each device.
///
/// It is a flat map, one entry per thing (`server|…`, `tv|…`,
/// `account|…`, `favourites`) to its details as JSON text, so two copies
/// are merged entry by entry against the copy both had last
/// (`merge(mine:theirs:base:)`): an addition, change or removal on either
/// side carries over, and a removal does not come back.
enum SharedSetup {
    typealias Snapshot = [String: String]

    /// The last copy both sides had, kept on this device.
    static let baseKey = "icloud_shared_base"

    // MARK: Reading

    static func snapshot(_ store: Store) throws -> Snapshot {
        var shot: Snapshot = [:]
        let off = try SwitchedOff.load(store)
        for server in try store.servers() {
            shot[serverKey(server.kind, server.address, server.userID)] = json([
                "name": server.name, "token": server.token, "off": off.servers.contains(server.id),
            ])
        }
        for source in try store.tvSources() {
            shot["tv|" + source.url] = json(["name": source.name, "off": off.tvSources.contains(source.url)])
        }
        if let favourites = try store.rawValue(LiveTV.favouritesKey), let value = parse(favourites),
           !((value as? [Any])?.isEmpty ?? false) {
            shot["favourites"] = json(value)
        }
        let accounts = (try store.rawValue(TrackedAccount.key)).flatMap(parse) as? [[String: Any]] ?? []
        for account in accounts {
            guard let service = account["service"] as? String else { continue }
            shot["account|" + service] = json(account)
        }
        return shot
    }

    static func serverKey(_ kind: String, _ address: String, _ user: String) -> String {
        "server|\(kind)|\(address)|\(user)"
    }

    // MARK: Merging

    /// Entry by entry: unchanged on one side since `base`, the other
    /// side's wins (a removal included); changed on both, this device's
    /// stays, except a sign-in, where the one that lasts longer wins.
    static func merge(mine: Snapshot, theirs: Snapshot, base: Snapshot) -> Snapshot {
        var result: Snapshot = [:]
        for key in Set(mine.keys).union(theirs.keys) {
            let (m, t, b) = (mine[key], theirs[key], base[key])
            let chosen: String?
            if m == t || t == b {
                chosen = m
            } else if m == b {
                chosen = t
            } else if key.hasPrefix("account|"), let m, let t {
                chosen = expires(t) > expires(m) ? t : m
            } else {
                // Changed on both sides (or added on both): keep this one,
                // unless it was removed here and changed there.
                chosen = m ?? t
            }
            if let chosen { result[key] = chosen }
        }
        return result
    }

    private static func expires(_ account: String) -> Int64 {
        ((parse(account) as? [String: Any])?["expires_at"] as? NSNumber)?.int64Value ?? 0
    }

    // MARK: Putting it in place

    /// What applying `wanted` over `mine` changed, for the models to reload.
    struct Applied: Equatable {
        var servers = false
        var tv = false
        var favourites = false
        var accounts = false
        var any: Bool { servers || tv || favourites || accounts }
    }

    /// Makes the store match `wanted` where it differs from `mine` (this
    /// device's snapshot).
    @discardableResult
    static func apply(_ wanted: Snapshot, over mine: Snapshot, into store: Store) throws -> Applied {
        var applied = Applied()
        var off = try SwitchedOff.load(store)
        let changed = Set(mine.keys).union(wanted.keys).filter { mine[$0] != wanted[$0] }

        let servers = try store.servers()
        for key in changed where key.hasPrefix("server|") {
            let parts = key.split(separator: "|", maxSplits: 3, omittingEmptySubsequences: false).map(String.init)
            guard parts.count == 4 else { continue }
            let (kind, address, user) = (parts[1], parts[2], parts[3])
            let here = servers.first { $0.kind == kind && $0.address == address && $0.userID == user }
            if let value = wanted[key].flatMap(object) {
                let id = try store.saveServer(StoredServer(kind: kind, name: value["name"] as? String ?? "", address: address,
                                                           userID: user, token: value["token"] as? String ?? ""))
                off.servers.removeAll { $0 == id }
                if value["off"] as? Bool == true { off.servers.append(id) }
            } else if let here {
                try store.removeServer(id: here.id)
                off.servers.removeAll { $0 == here.id }
            }
            applied.servers = true
        }
        for key in changed where key.hasPrefix("tv|") {
            let url = String(key.dropFirst(3))
            if let value = wanted[key].flatMap(object) {
                try store.saveTvSource(TvSource(url: url, name: value["name"] as? String ?? "", playlist: ""))
                off.tvSources.removeAll { $0 == url }
                if value["off"] as? Bool == true { off.tvSources.append(url) }
            } else {
                try store.removeTvSource(url: url)
                off.tvSources.removeAll { $0 == url }
            }
            applied.tv = true
        }
        if applied.servers || applied.tv { try off.save(store) }

        if changed.contains("favourites") {
            if let favourites = wanted["favourites"] {
                try store.putRaw(LiveTV.favouritesKey, favourites)
            } else {
                try store.remove(LiveTV.favouritesKey)
            }
            applied.favourites = true
        }
        if changed.contains(where: { $0.hasPrefix("account|") }) {
            let accounts = wanted.filter { $0.key.hasPrefix("account|") }.sorted { $0.key < $1.key }.compactMap { parse($0.value) }
            let data = try JSONSerialization.data(withJSONObject: accounts, options: [.withoutEscapingSlashes])
            try store.putRaw(TrackedAccount.key, String(decoding: data, as: UTF8.self))
            applied.accounts = true
        }
        return applied
    }

    // MARK: Text

    static func encode(_ shot: Snapshot) -> String {
        json(shot)
    }

    static func decode(_ text: String?) -> Snapshot? {
        guard let text, let value = parse(text) as? [String: String] else { return nil }
        return value
    }

    /// JSON with sorted keys, so the same details always read the same.
    private static func json(_ value: Any) -> String {
        guard let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys, .withoutEscapingSlashes, .fragmentsAllowed])
        else { return "" }
        return String(decoding: data, as: UTF8.self)
    }

    private static func parse(_ text: String) -> Any? {
        try? JSONSerialization.jsonObject(with: Data(text.utf8), options: [.fragmentsAllowed])
    }

    private static func object(_ text: String) -> [String: Any]? {
        parse(text) as? [String: Any]
    }
}
