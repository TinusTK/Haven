import Foundation

/// A backup of Haven: the whole setup (media servers and their sign-ins,
/// add-ons in the GitHub build, Trakt and Simkl, keys, TV sources, the
/// Watchlist, what was watched, what is switched off) and the settings, as
/// one JSON file, `Haven backup.json`. The same file as the Rust build's
/// (`haven-core` `backup.rs` and `setup.rs`), so a backup moves between the
/// old app, the new one and other devices. Restoring adds what is missing
/// and never removes anything.
enum Backup {
    static let fileName = "Haven backup.json"
    static let kind = "Haven backup"
    /// Settings that travel with a backup (others belong to one device).
    static let settings = ["settings", "sync_options", "external_player", "country", "tv_favourites", "player", "update_check_at_start"]
    /// When the last backup was made, in seconds since 1970.
    static let lastKey = "backup_last"
    /// How often the backup is brought up to date while Haven is open,
    /// and how long the first one waits after start.
    static let every: TimeInterval = 10 * 60
    static let afterStart: TimeInterval = 30

    /// What a restore added.
    struct Merged: Equatable, Sendable {
        var added = 0
        /// A Trakt or Simkl sign-in was added or replaced by a newer one.
        var tracking = false
    }

    enum Failure: Error, Equatable {
        case notABackup
    }

    // MARK: Making one

    /// The backup of `store` as JSON, made at `now` (seconds since 1970).
    /// `sources`: add-ons go in (only builds that have them).
    static func make(_ store: Store, sources: Bool = Build.hasAddons, now: Int64) throws -> [String: Any] {
        var kept: [String: Any] = [:]
        for key in settings {
            if let raw = try store.rawValue(key), let value = parse(raw) { kept[key] = value }
        }
        return ["kind": kind, "version": 1, "made_at": now, "setup": try setup(store, sources: sources), "settings": kept]
    }

    /// The setup part (what `Setup::read` gives in the Rust build).
    static func setup(_ store: Store, sources: Bool) throws -> [String: Any] {
        let off = try SwitchedOff.load(store)
        let servers = try store.servers()
        var addons: [[String: Any]] = []
        #if ADDONS
        if sources { addons = try store.addons().map { ["url": $0.url, "description": $0.manifest] } }
        #endif
        let keys = try Keys.load(store)
        let tracking = (try store.rawValue(TrackedAccount.key)).flatMap(parse) as? [Any] ?? []
        return [
            "servers": servers.map { ["kind": $0.kind, "name": $0.name, "address": $0.address, "user_id": $0.userID, "token": $0.token] },
            "sources": addons,
            "tv_sources": try store.tvSources().map { ["url": $0.url, "name": $0.name] },
            "keys": ["tmdb": keys.tmdb, "trakt_id": keys.traktID, "trakt_secret": keys.traktSecret, "simkl_id": keys.simklID, "omdb": keys.omdb],
            "watchlist": try store.watchlist().map {
                ["kind": $0.kind, "title_id": $0.titleID, "title": $0.title, "year": $0.year, "poster": $0.poster.map { $0 as Any } ?? NSNull(), "added_at": $0.addedAt]
            },
            "progress": try store.allProgress().map {
                ["kind": $0.kind, "title_id": $0.titleID, "video_id": $0.videoID, "title": $0.title, "detail": $0.detail,
                 "poster": $0.poster.map { $0 as Any } ?? NSNull(), "position": $0.position, "duration": $0.duration, "finished": $0.finished,
                 "updated_at": $0.updatedAt]
            },
            "tracking": tracking,
            "off": [
                "servers": servers.filter { off.servers.contains($0.id) }.map { ["kind": $0.kind, "address": $0.address] },
                "addons": sources ? off.addons : [],
                "tv_sources": off.tvSources,
            ],
        ]
    }

    /// The text written to the file.
    static func text(_ backup: [String: Any]) throws -> String {
        let data = try JSONSerialization.data(withJSONObject: backup, options: [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes])
        return String(decoding: data, as: UTF8.self)
    }

    /// What decides whether the backup changed: the setup and settings,
    /// without the time it was made.
    static func contents(_ backup: [String: Any]) -> String {
        let parts: [String: Any] = ["setup": backup["setup"] ?? [:], "settings": backup["settings"] ?? [:]]
        return (try? JSONSerialization.data(withJSONObject: parts, options: [.sortedKeys])).map { String(decoding: $0, as: UTF8.self) } ?? ""
    }

    /// Writes it to `url`, replacing the file only once the new one is
    /// complete (a crash halfway leaves the old backup whole).
    static func write(_ backup: [String: Any], to url: URL) throws {
        try write(text: try text(backup), to: url)
    }

    static func write(text: String, to url: URL) throws {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let partial = url.deletingPathExtension().appendingPathExtension("json.part")
        try Data(text.utf8).write(to: partial)
        if FileManager.default.fileExists(atPath: url.path) {
            _ = try FileManager.default.replaceItemAt(url, withItemAt: partial)
        } else {
            try FileManager.default.moveItem(at: partial, to: url)
        }
    }

    // MARK: Reading one

    private static func parse(_ text: String) -> Any? {
        try? JSONSerialization.jsonObject(with: Data(text.utf8), options: [.fragmentsAllowed])
    }

    /// Reads a backup's text; fails if it is not a Haven backup.
    static func read(_ text: String) throws -> [String: Any] {
        guard let backup = parse(text) as? [String: Any], backup["kind"] as? String == kind else { throw Failure.notABackup }
        return backup
    }

    /// When a backup was made, in seconds since 1970.
    static func madeAt(_ backup: [String: Any]) -> Int64 {
        (backup["made_at"] as? NSNumber)?.int64Value ?? 0
    }

    // MARK: Restoring

    /// Adds a backup to `store`. Settings are put back only where the store
    /// has none of its own yet (a fresh install). Nothing is removed.
    static func restore(_ backup: [String: Any], into store: Store, sources: Bool = Build.hasAddons) throws -> Merged {
        for (key, value) in backup["settings"] as? [String: Any] ?? [:] where settings.contains(key) {
            guard try store.rawValue(key) == nil else { continue }
            let data = try JSONSerialization.data(withJSONObject: value, options: [.fragmentsAllowed, .withoutEscapingSlashes])
            try store.putRaw(key, String(decoding: data, as: UTF8.self))
        }
        return try merge(backup["setup"] as? [String: Any] ?? [:], into: store, sources: sources)
    }

    private static func text(_ value: Any?) -> String { value as? String ?? "" }
    private static func whole(_ value: Any?) -> Int64 { (value as? NSNumber)?.int64Value ?? 0 }
    private static func real(_ value: Any?) -> Double { (value as? NSNumber)?.doubleValue ?? 0 }
    private static func list(_ value: Any?) -> [[String: Any]] { value as? [[String: Any]] ?? [] }

    /// What `Setup::merge_into` does in the Rust build: servers, add-ons and
    /// TV sources already here by address are kept; what arrives switched
    /// off stays off; keys only fill in what is blank; Watchlist entries are
    /// added; for progress the newer record wins; a Trakt or Simkl sign-in
    /// is taken where there is none here, or an older one.
    static func merge(_ setup: [String: Any], into store: Store, sources: Bool) throws -> Merged {
        var merged = Merged()
        let offRecord = setup["off"] as? [String: Any] ?? [:]
        let offServers = list(offRecord["servers"]).map { (text($0["kind"]), text($0["address"])) }
        let offAddons = offRecord["addons"] as? [String] ?? []
        let offTV = offRecord["tv_sources"] as? [String] ?? []
        var off = try SwitchedOff.load(store)
        var offChanged = false

        let have = try store.servers()
        for server in list(setup["servers"]) {
            let (kind, address) = (text(server["kind"]), text(server["address"]))
            if have.contains(where: { $0.address == address && $0.kind == kind }) { continue }
            let id = try store.saveServer(StoredServer(kind: kind, name: text(server["name"]), address: address,
                                                       userID: text(server["user_id"]), token: text(server["token"])))
            if offServers.contains(where: { $0 == (kind, address) }) {
                off.servers.append(id)
                offChanged = true
            }
            merged.added += 1
        }
        #if ADDONS
        if sources {
            let installed = try store.addons()
            for addon in list(setup["sources"]) {
                let url = text(addon["url"])
                guard !installed.contains(where: { $0.url == url }) else { continue }
                try store.saveAddon(url: url, manifest: text(addon["description"]))
                if offAddons.contains(url) && !off.addons.contains(url) {
                    off.addons.append(url)
                    offChanged = true
                }
                merged.added += 1
            }
        }
        #endif
        _ = offAddons
        let tv = try store.tvSources()
        for source in list(setup["tv_sources"]) {
            let url = text(source["url"])
            guard !tv.contains(where: { $0.url == url }) else { continue }
            try store.saveTvSource(TvSource(url: url, name: text(source["name"]), playlist: ""))
            if offTV.contains(url) && !off.tvSources.contains(url) {
                off.tvSources.append(url)
                offChanged = true
            }
            merged.added += 1
        }
        if offChanged { try off.save(store) }

        if let keys = setup["keys"] as? [String: Any] {
            var mine = try Keys.load(store)
            func fill(_ value: inout String, _ theirs: Any?) {
                let theirs = text(theirs)
                if value.trimmingCharacters(in: .whitespaces).isEmpty && !theirs.trimmingCharacters(in: .whitespaces).isEmpty { value = theirs }
            }
            fill(&mine.tmdb, keys["tmdb"])
            fill(&mine.traktID, keys["trakt_id"])
            fill(&mine.traktSecret, keys["trakt_secret"])
            fill(&mine.simklID, keys["simkl_id"])
            fill(&mine.omdb, keys["omdb"])
            try mine.save(store)
        }
        for saved in list(setup["watchlist"]) {
            let (kind, id) = (text(saved["kind"]), text(saved["title_id"]))
            guard !(try store.inWatchlist(kind: kind, titleID: id)) else { continue }
            try store.addToWatchlist(Saved(kind: kind, titleID: id, title: text(saved["title"]), year: text(saved["year"]),
                                           poster: saved["poster"] as? String, addedAt: whole(saved["added_at"])))
            merged.added += 1
        }
        for record in list(setup["progress"]) {
            let progress = WatchProgress(kind: text(record["kind"]), titleID: text(record["title_id"]), videoID: text(record["video_id"]),
                                         title: text(record["title"]), detail: text(record["detail"]), poster: record["poster"] as? String,
                                         position: real(record["position"]), duration: real(record["duration"]),
                                         finished: record["finished"] as? Bool ?? false, updatedAt: whole(record["updated_at"]))
            let mine = try store.progress(kind: progress.kind, titleID: progress.titleID, videoID: progress.videoID)
            guard mine.map({ progress.updatedAt > $0.updatedAt }) ?? true else { continue }
            try store.saveProgress(progress)
            merged.added += 1
        }
        let theirs = setup["tracking"] as? [[String: Any]] ?? []
        if !theirs.isEmpty {
            var mine = (try store.rawValue(TrackedAccount.key)).flatMap(parse) as? [[String: Any]] ?? []
            func service(_ account: [String: Any]) -> String? { account["service"] as? String }
            func expires(_ account: [String: Any]) -> Int64 { whole(account["expires_at"]) }
            for account in theirs {
                guard let name = service(account) else { continue }
                if let at = mine.firstIndex(where: { service($0) == name }) {
                    if expires(account) > expires(mine[at]) {
                        mine[at] = account
                        merged.tracking = true
                    }
                } else {
                    mine.append(account)
                    merged.tracking = true
                    merged.added += 1
                }
            }
            if merged.tracking {
                let data = try JSONSerialization.data(withJSONObject: mine, options: [.withoutEscapingSlashes])
                try store.putRaw(TrackedAccount.key, String(decoding: data, as: UTF8.self))
            }
        }
        return merged
    }

    // MARK: Where it goes

    /// The folder and how to say where it is: iCloud Drive › Haven in the
    /// App Store build (the Files app when iCloud is off), else Files › On
    /// My iPhone › Haven. Looking up iCloud can take a moment: not on the
    /// main thread.
    static func folder() -> (url: URL, words: String) {
        #if STORE
        if let cloud = FileManager.default.url(forUbiquityContainerIdentifier: nil) {
            return (cloud.appendingPathComponent("Documents", isDirectory: true), "iCloud Drive › Haven")
        }
        #endif
        return (DownloadRules.documents, "Files › On My iPhone › Haven")
    }

    /// "today at 21:40", or "3 Nov at 09:15".
    static func when(_ at: Int64, now: Date = Date(), zone: TimeZone = .current) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(at))
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = zone
        let format = DateFormatter()
        format.timeZone = zone
        format.locale = Locale(identifier: "en_GB")
        if calendar.isDate(date, inSameDayAs: now) {
            format.dateFormat = "HH:mm"
            return "today at \(format.string(from: date))"
        }
        format.dateFormat = "d MMM 'at' HH:mm"
        return format.string(from: date)
    }

    static let firstNote = "Haven backs up your media servers, accounts, Trakt and Simkl, keys, Watchlist, what you watched and your settings while it is open."

    /// "iCloud Drive" or "The Files app": where the backup goes, for its row.
    static var whereWords: String { Build.isStore ? "iCloud Drive" : "The Files app" }
}
