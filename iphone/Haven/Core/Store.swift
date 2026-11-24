import Foundation
import GRDB

/// Haven's local SQLite database (`haven.sqlite3`).
///
/// The schema is the Rust build's, step for step: the same tables, the same
/// `PRAGMA user_version` numbering, so a phone that updates from the Rust
/// build keeps everything, and each build refuses a database made by a
/// newer one. Never edit a step that has shipped: add a new one.
final class Store: Sendable {
    /// Schema steps, applied in order. The index + 1 is the schema version.
    static let migrations: [String] = [
        """
        CREATE TABLE kv (
            key   TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL
        ) STRICT;
        """,
        """
        CREATE TABLE addons (
            url      TEXT PRIMARY KEY NOT NULL,
            manifest TEXT NOT NULL,
            position INTEGER NOT NULL
        ) STRICT;
        """,
        """
        CREATE TABLE progress (
            kind       TEXT NOT NULL,
            title_id   TEXT NOT NULL,
            video_id   TEXT NOT NULL,
            title      TEXT NOT NULL,
            detail     TEXT NOT NULL,
            poster     TEXT,
            position   REAL NOT NULL,
            duration   REAL NOT NULL,
            finished   INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY (kind, title_id, video_id)
        ) STRICT;
        CREATE INDEX progress_recent ON progress (updated_at DESC);
        """,
        """
        CREATE TABLE watchlist (
            kind     TEXT NOT NULL,
            title_id TEXT NOT NULL,
            title    TEXT NOT NULL,
            year     TEXT NOT NULL,
            poster   TEXT,
            added_at INTEGER NOT NULL,
            PRIMARY KEY (kind, title_id)
        ) STRICT;
        """,
        """
        CREATE TABLE tv_sources (
            url      TEXT PRIMARY KEY NOT NULL,
            name     TEXT NOT NULL,
            playlist TEXT NOT NULL,
            position INTEGER NOT NULL
        ) STRICT;
        """,
        """
        CREATE TABLE servers (
            id      INTEGER PRIMARY KEY AUTOINCREMENT,
            kind    TEXT NOT NULL,
            name    TEXT NOT NULL,
            address TEXT NOT NULL,
            user_id TEXT NOT NULL,
            token   TEXT NOT NULL,
            UNIQUE (kind, address, user_id)
        ) STRICT;
        """,
        """
        CREATE TABLE downloads (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            kind       TEXT NOT NULL,
            title_id   TEXT NOT NULL,
            video_id   TEXT NOT NULL,
            title      TEXT NOT NULL,
            detail     TEXT NOT NULL,
            season     INTEGER,
            episode    INTEGER,
            poster     TEXT,
            url        TEXT NOT NULL,
            path       TEXT NOT NULL,
            size       INTEGER NOT NULL,
            received   INTEGER NOT NULL,
            state      TEXT NOT NULL,
            added_at   INTEGER NOT NULL
        ) STRICT;
        """,
        """
        CREATE TABLE sync_queue (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            what       TEXT NOT NULL,
            kind       TEXT NOT NULL,
            title_id   TEXT NOT NULL,
            video_id   TEXT NOT NULL,
            created_at INTEGER NOT NULL
        ) STRICT;
        """,
        // Haven Hub is gone (0.2.4): forget any pairing with one.
        "DELETE FROM servers WHERE kind = 'hub';",
    ]

    private let db: DatabaseQueue

    private init(_ db: DatabaseQueue) throws {
        self.db = db
        try Self.migrate(db)
    }

    static func open(_ url: URL) throws -> Store {
        try Store(guarded { try DatabaseQueue(path: url.path, configuration: configuration) })
    }

    static func inMemory() throws -> Store {
        try Store(guarded { try DatabaseQueue(configuration: configuration) })
    }

    private static var configuration: Configuration {
        var config = Configuration()
        config.foreignKeysEnabled = true
        return config
    }

    /// Brings the database up to the newest step, one step per transaction.
    static func migrate(_ db: DatabaseQueue) throws {
        let known = migrations.count
        let found = try guarded { try db.read { try Int.fetchOne($0, sql: "PRAGMA user_version") ?? 0 } }
        if found > known {
            throw HavenError.databaseTooNew(found: found, known: known)
        }
        for index in found ..< known {
            try guarded {
                try db.write { d in
                    try d.execute(sql: migrations[index])
                    try d.execute(sql: "PRAGMA user_version = \(index + 1)")
                }
            }
        }
    }

    func schemaVersion() throws -> Int {
        try read { try Int.fetchOne($0, sql: "PRAGMA user_version") ?? 0 }
    }

    // MARK: Saved values (kv)

    /// A value saved with `put`; nil if never saved.
    func get<T: Decodable>(_ key: String, as type: T.Type = T.self) throws -> T? {
        guard let json = try read({ try String.fetchOne($0, sql: "SELECT value FROM kv WHERE key = ?", arguments: [key]) }) else {
            return nil
        }
        do {
            return try JSONDecoder().decode(T.self, from: Data(json.utf8))
        } catch {
            throw HavenError.json(String(describing: error))
        }
    }

    func put<T: Encodable>(_ key: String, _ value: T) throws {
        let json: String
        do {
            json = String(decoding: try Self.encoder.encode(value), as: UTF8.self)
        } catch {
            throw HavenError.json(String(describing: error))
        }
        try write {
            try $0.execute(
                sql: """
                INSERT INTO kv (key, value) VALUES (?, ?)
                ON CONFLICT(key) DO UPDATE SET value = excluded.value
                """,
                arguments: [key, json])
        }
    }

    func remove(_ key: String) throws {
        try write { try $0.execute(sql: "DELETE FROM kv WHERE key = ?", arguments: [key]) }
    }

    /// JSON as the Rust build wrote it: compact, slashes not escaped.
    static let encoder: JSONEncoder = {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.withoutEscapingSlashes]
        return encoder
    }()

    // MARK: Add-ons (GitHub build only; the App Store build has the
    // table, so databases match, but no code that uses it)

#if ADDONS
    /// Installed add-ons, in the order they were installed.
    func addons() throws -> [StoredAddon] {
        try read {
            try Row.fetchAll($0, sql: "SELECT url, manifest FROM addons ORDER BY position, url")
                .map { StoredAddon(url: $0[0], manifest: $0[1]) }
        }
    }

    /// Adds an add-on at the end, or refreshes the manifest of one already
    /// installed (it keeps its place).
    func saveAddon(url: String, manifest: String) throws {
        try write {
            try $0.execute(
                sql: """
                INSERT INTO addons (url, manifest, position)
                VALUES (?, ?, (SELECT COALESCE(MAX(position), 0) + 1 FROM addons))
                ON CONFLICT(url) DO UPDATE SET manifest = excluded.manifest
                """,
                arguments: [url, manifest])
        }
    }

    func removeAddon(url: String) throws {
        try write { try $0.execute(sql: "DELETE FROM addons WHERE url = ?", arguments: [url]) }
    }
#endif

    // MARK: Progress

    private static let progressColumns =
        "kind, title_id, video_id, title, detail, poster, position, duration, finished, updated_at"

    private static func progress(_ row: Row) -> WatchProgress {
        WatchProgress(
            kind: row[0], titleID: row[1], videoID: row[2], title: row[3], detail: row[4],
            poster: row[5], position: row[6], duration: row[7], finished: row[8], updatedAt: row[9])
    }

    /// Saves how far a film or episode was watched, replacing what was saved
    /// for it before. A poster already on record is kept when the new entry
    /// has none.
    func saveProgress(_ p: WatchProgress) throws {
        try write {
            try $0.execute(
                sql: """
                INSERT INTO progress (kind, title_id, video_id, title, detail, poster,
                                      position, duration, finished, updated_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(kind, title_id, video_id) DO UPDATE SET
                   title = excluded.title,
                   detail = excluded.detail,
                   poster = COALESCE(excluded.poster, poster),
                   position = excluded.position,
                   duration = excluded.duration,
                   finished = excluded.finished,
                   updated_at = excluded.updated_at
                """,
                arguments: [p.kind, p.titleID, p.videoID, p.title, p.detail, p.poster,
                            p.position, p.duration, p.finished, p.updatedAt])
        }
    }

    /// Forgets one film or episode (marked unwatched).
    func removeVideoProgress(kind: String, titleID: String, videoID: String) throws {
        try write {
            try $0.execute(
                sql: "DELETE FROM progress WHERE kind = ? AND title_id = ? AND video_id = ?",
                arguments: [kind, titleID, videoID])
        }
    }

    /// What was saved for one film or episode, if anything.
    func progress(kind: String, titleID: String, videoID: String) throws -> WatchProgress? {
        try read {
            try Row.fetchOne(
                $0,
                sql: "SELECT \(Self.progressColumns) FROM progress WHERE kind = ? AND title_id = ? AND video_id = ?",
                arguments: [kind, titleID, videoID]
            ).map(Self.progress)
        }
    }

    /// For each title, the row saved last (ties: the larger video id).
    private static let latestPerTitle = """
        NOT EXISTS (
            SELECT 1 FROM progress AS q
            WHERE q.kind = p.kind AND q.title_id = p.title_id
              AND (q.updated_at > p.updated_at
                   OR (q.updated_at = p.updated_at AND q.video_id > p.video_id)))
        """

    /// Titles to carry on with, most recent first: for each title the film
    /// or episode watched last, unless that one was finished.
    func continueWatching(limit: Int) throws -> [WatchProgress] {
        try read {
            try Row.fetchAll(
                $0,
                sql: """
                SELECT \(Self.progressColumns) FROM progress AS p
                WHERE finished = 0 AND \(Self.latestPerTitle)
                ORDER BY updated_at DESC, title_id
                LIMIT ?
                """,
                arguments: [limit]
            ).map(Self.progress)
        }
    }

    /// Everything watched, newest first: for each title the film or episode
    /// watched last, finished or not.
    func history(limit: Int) throws -> [WatchProgress] {
        try read {
            try Row.fetchAll(
                $0,
                sql: """
                SELECT \(Self.progressColumns) FROM progress AS p
                WHERE \(Self.latestPerTitle)
                ORDER BY updated_at DESC, title_id
                LIMIT ?
                """,
                arguments: [limit]
            ).map(Self.progress)
        }
    }

    /// Every film and episode with a position or a watched mark, newest
    /// first (for a backup).
    func allProgress() throws -> [WatchProgress] {
        try read {
            try Row.fetchAll($0, sql: "SELECT \(Self.progressColumns) FROM progress ORDER BY updated_at DESC")
                .map(Self.progress)
        }
    }

    /// The films and episodes of one title watched to the end.
    func finishedVideos(kind: String, titleID: String) throws -> [String] {
        try read {
            try String.fetchAll(
                $0,
                sql: "SELECT video_id FROM progress WHERE kind = ? AND title_id = ? AND finished = 1",
                arguments: [kind, titleID])
        }
    }

    /// Records something watched elsewhere (brought in from a tracking
    /// service) unless Haven already has a record of it. True if added.
    @discardableResult
    func addWatchedElsewhere(_ p: WatchProgress) throws -> Bool {
        try write { d in
            try d.execute(
                sql: """
                INSERT OR IGNORE INTO progress (kind, title_id, video_id, title, detail, poster,
                                                position, duration, finished, updated_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, ?)
                """,
                arguments: [p.kind, p.titleID, p.videoID, p.title, p.detail, p.poster,
                            p.position, p.duration, p.updatedAt])
            return d.changesCount > 0
        }
    }

    /// Forgets everything watched of one title (all its episodes).
    func removeProgress(kind: String, titleID: String) throws {
        try write {
            try $0.execute(sql: "DELETE FROM progress WHERE kind = ? AND title_id = ?", arguments: [kind, titleID])
        }
    }

    // MARK: Media servers

    /// The media servers, in the order they were added.
    func servers() throws -> [StoredServer] {
        try read {
            try Row.fetchAll($0, sql: "SELECT id, kind, name, address, user_id, token FROM servers ORDER BY id")
                .map { StoredServer(id: $0[0], kind: $0[1], name: $0[2], address: $0[3], userID: $0[4], token: $0[5]) }
        }
    }

    /// Adds a server, or refreshes the name and token of one already there
    /// (same kind, address and user). Returns its number.
    @discardableResult
    func saveServer(_ s: StoredServer) throws -> Int64 {
        try write {
            try Int64.fetchOne(
                $0,
                sql: """
                INSERT INTO servers (kind, name, address, user_id, token)
                VALUES (?, ?, ?, ?, ?)
                ON CONFLICT(kind, address, user_id) DO UPDATE SET
                   name = excluded.name, token = excluded.token
                RETURNING id
                """,
                arguments: [s.kind, s.name, s.address, s.userID, s.token]) ?? 0
        }
    }

    func removeServer(id: Int64) throws {
        try write { try $0.execute(sql: "DELETE FROM servers WHERE id = ?", arguments: [id]) }
    }

    // MARK: Live TV sources

    /// Television and radio sources, in the order they were added.
    func tvSources() throws -> [TvSource] {
        try read {
            try Row.fetchAll($0, sql: "SELECT url, name, playlist FROM tv_sources ORDER BY position, url")
                .map { TvSource(url: $0[0], name: $0[1], playlist: $0[2]) }
        }
    }

    /// Adds a source at the end, or refreshes the playlist of one already
    /// there (it keeps its place and name).
    func saveTvSource(_ s: TvSource) throws {
        try write {
            try $0.execute(
                sql: """
                INSERT INTO tv_sources (url, name, playlist, position)
                VALUES (?, ?, ?, (SELECT COALESCE(MAX(position), 0) + 1 FROM tv_sources))
                ON CONFLICT(url) DO UPDATE SET playlist = excluded.playlist
                """,
                arguments: [s.url, s.name, s.playlist])
        }
    }

    func removeTvSource(url: String) throws {
        try write { try $0.execute(sql: "DELETE FROM tv_sources WHERE url = ?", arguments: [url]) }
    }

    // MARK: Watchlist

    /// The watchlist, newest first.
    func watchlist() throws -> [Saved] {
        try read {
            try Row.fetchAll(
                $0,
                sql: "SELECT kind, title_id, title, year, poster, added_at FROM watchlist ORDER BY added_at DESC, title"
            ).map { Saved(kind: $0[0], titleID: $0[1], title: $0[2], year: $0[3], poster: $0[4], addedAt: $0[5]) }
        }
    }

    /// Saves a title to watch later. Saving it again keeps its place in the
    /// list and refreshes its name, year and poster.
    func addToWatchlist(_ s: Saved) throws {
        try write {
            try $0.execute(
                sql: """
                INSERT INTO watchlist (kind, title_id, title, year, poster, added_at)
                VALUES (?, ?, ?, ?, ?, ?)
                ON CONFLICT(kind, title_id) DO UPDATE SET
                   title = excluded.title,
                   year = excluded.year,
                   poster = COALESCE(excluded.poster, poster)
                """,
                arguments: [s.kind, s.titleID, s.title, s.year, s.poster, s.addedAt])
        }
    }

    func removeFromWatchlist(kind: String, titleID: String) throws {
        try write {
            try $0.execute(sql: "DELETE FROM watchlist WHERE kind = ? AND title_id = ?", arguments: [kind, titleID])
        }
    }

    func inWatchlist(kind: String, titleID: String) throws -> Bool {
        try read {
            try Bool.fetchOne(
                $0,
                sql: "SELECT EXISTS (SELECT 1 FROM watchlist WHERE kind = ? AND title_id = ?)",
                arguments: [kind, titleID]) ?? false
        }
    }

    // MARK: Sync queue

    /// Notes a change to send. A Watchlist change cancels a waiting opposite
    /// one for the same title; watched / unwatched for the same video: the
    /// last one counts; the same change is kept once.
    func queueChange(_ c: Change) throws {
        try write { d in
            if c.what.hasPrefix("watchlist_") {
                try d.execute(
                    sql: "DELETE FROM sync_queue WHERE what LIKE 'watchlist_%' AND kind = ? AND title_id = ?",
                    arguments: [c.kind, c.titleID])
            } else if c.what == "watched" || c.what == "unwatched" {
                try d.execute(
                    sql: """
                    DELETE FROM sync_queue WHERE what IN ('watched', 'unwatched')
                      AND kind = ? AND title_id = ? AND video_id = ?
                    """,
                    arguments: [c.kind, c.titleID, c.videoID])
            } else {
                try d.execute(
                    sql: "DELETE FROM sync_queue WHERE what = ? AND kind = ? AND title_id = ? AND video_id = ?",
                    arguments: [c.what, c.kind, c.titleID, c.videoID])
            }
            try d.execute(
                sql: "INSERT INTO sync_queue (what, kind, title_id, video_id, created_at) VALUES (?, ?, ?, ?, ?)",
                arguments: [c.what, c.kind, c.titleID, c.videoID, c.createdAt])
        }
    }

    /// The changes still to send, oldest first.
    func queuedChanges() throws -> [Change] {
        try read {
            try Row.fetchAll($0, sql: "SELECT id, what, kind, title_id, video_id, created_at FROM sync_queue ORDER BY id")
                .map { Change(id: $0[0], what: $0[1], kind: $0[2], titleID: $0[3], videoID: $0[4], createdAt: $0[5]) }
        }
    }

    func removeChange(id: Int64) throws {
        try write { try $0.execute(sql: "DELETE FROM sync_queue WHERE id = ?", arguments: [id]) }
    }

    // MARK: Downloads

    private static let downloadColumns =
        "id, kind, title_id, video_id, title, detail, season, episode, poster, url, path, size, received, state, added_at"

    private static func download(_ r: Row) -> Download {
        Download(
            id: r[0], kind: r[1], titleID: r[2], videoID: r[3], title: r[4], detail: r[5],
            season: r[6], episode: r[7], poster: r[8], url: r[9], path: r[10], size: r[11],
            received: r[12], state: r[13], addedAt: r[14])
    }

    /// Every download: films and series by name, episodes in order.
    func downloads() throws -> [Download] {
        try read {
            try Row.fetchAll(
                $0,
                sql: """
                SELECT \(Self.downloadColumns) FROM downloads
                ORDER BY kind, title COLLATE NOCASE, title_id, season, episode, added_at
                """
            ).map(Self.download)
        }
    }

    /// Adds a download; returns its number.
    @discardableResult
    func addDownload(_ d: Download) throws -> Int64 {
        try write {
            try Int64.fetchOne(
                $0,
                sql: """
                INSERT INTO downloads (kind, title_id, video_id, title, detail, season, episode,
                                       poster, url, path, size, received, state, added_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                RETURNING id
                """,
                arguments: [d.kind, d.titleID, d.videoID, d.title, d.detail, d.season, d.episode,
                            d.poster, d.url, d.path, d.size, d.received, d.state, d.addedAt]) ?? 0
        }
    }

    /// Records how far a download has got, and its state.
    func updateDownload(id: Int64, size: Int64, received: Int64, state: String) throws {
        try write {
            try $0.execute(
                sql: "UPDATE downloads SET size = ?, received = ?, state = ? WHERE id = ?",
                arguments: [size, received, state, id])
        }
    }

    /// Changes where a download is saved (once its file type is known).
    func setDownloadPath(id: Int64, path: String) throws {
        try write { try $0.execute(sql: "UPDATE downloads SET path = ? WHERE id = ?", arguments: [path, id]) }
    }

    func removeDownload(id: Int64) throws {
        try write { try $0.execute(sql: "DELETE FROM downloads WHERE id = ?", arguments: [id]) }
    }

    // MARK: Plumbing

    private func read<T>(_ body: (Database) throws -> T) throws -> T {
        try Self.guarded { try db.read(body) }
    }

    private func write<T>(_ body: (Database) throws -> T) throws -> T {
        try Self.guarded { try db.write(body) }
    }

    /// Turns GRDB's errors into Haven's.
    private static func guarded<T>(_ body: () throws -> T) throws -> T {
        do {
            return try body()
        } catch let error as HavenError {
            throw error
        } catch {
            throw HavenError.database(String(describing: error))
        }
    }
}
