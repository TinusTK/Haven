import CryptoKit
import Foundation

/// What Haven loaded last time, kept on disk so the next start shows it at
/// once: Home's shelves, Explore's first pages, the media servers' libraries,
/// titles' details and OMDb's scores. Each is asked for again in the
/// background and replaced when the fresh answer differs; a fresh answer
/// that fails keeps what was kept, and no error shows.
struct Kept: Sendable {
    let dir: URL

    /// The file a kept answer lives in, named after what it is.
    func file(_ name: String) -> URL {
        dir.appendingPathComponent(Cache.hashName(name) + ".json")
    }

    /// A kept answer; anything missing or unreadable is nil.
    func read<T: Decodable>(_ name: String, as type: T.Type = T.self) -> T? {
        guard let data = try? Data(contentsOf: file(name)) else { return nil }
        return try? JSONDecoder().decode(T.self, from: data)
    }

    /// Writes a kept answer (whole or not at all).
    func write<T: Encodable>(_ name: String, _ value: T) {
        guard let data = try? JSONEncoder().encode(value) else { return }
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try? data.write(to: file(name), options: .atomic)
    }

    /// Keeps a fresh answer. True when it differs from what was kept (what
    /// is on screen should then be replaced).
    @discardableResult
    func freshen<T: Codable & Equatable>(_ name: String, _ fresh: T) -> Bool {
        if read(name, as: T.self) == fresh { return false }
        write(name, fresh)
        return true
    }

    /// Forgets everything kept (Settings' Clear, with the pictures).
    func clear() {
        try? FileManager.default.removeItem(at: dir)
    }

    // MARK: Names, as the Rust build named them

    static func shelf(board: String, title: String, source: String) -> String { "shelf|\(board)|\(title)|\(source)" }
    static func explore(label: String, kind: String, genre: String) -> String { "explore|\(label)|\(kind)|\(genre)" }
    static func libraries(_ server: Server) -> String { ServerAccounts.librariesName(server) }
    static func details(kind: String, id: String) -> String { "details|\(kind)|\(id)" }
    static func omdb(_ imdb: String) -> String { "omdb|\(imdb)" }

    // MARK: OMDb scores, kept a week (the free key allows 1,000 a day)

    static let omdbKeptFor: Int64 = 7 * 86_400

    struct KeptScores: Codable, Equatable, Sendable {
        /// When they were fetched, in seconds since 1970.
        var at: Int64
        var scores: OMDb.Scores
    }

    /// A title's OMDb scores: kept ones while under a week old, else fresh
    /// ones (then kept); the kept ones when fresh ones cannot be had.
    func scores(imdb: String, key: String?, now: Int64, http: HTTP = .shared) async -> OMDb.Scores? {
        let kept = read(Self.omdb(imdb), as: KeptScores.self)
        if let kept, now - kept.at < Self.omdbKeptFor { return kept.scores }
        guard let key, let found = try? await OMDb.scores(key: key, imdb: imdb, http: http) else { return kept?.scores }
        write(Self.omdb(imdb), KeptScores(at: now, scores: found))
        return found
    }
}

/// Where Haven's cache lives: the system's Caches folder (left out of the
/// phone's backups, and emptied by iOS when space runs short).
enum Cache {
    static var root: URL {
        let caches = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first
            ?? FileManager.default.temporaryDirectory
        return caches.appendingPathComponent("Haven", isDirectory: true)
    }

    static var pictures: URL { root.appendingPathComponent("pictures", isDirectory: true) }
    static var kept: Kept { Kept(dir: root.appendingPathComponent("kept", isDirectory: true)) }

    /// A short file name for any text: the first 16 bytes of its SHA-256.
    static func hashName(_ text: String) -> String {
        SHA256.hash(data: Data(text.utf8)).prefix(16).map { String(format: "%02x", $0) }.joined()
    }

    /// The Rust build kept its pictures and answers in the data folder
    /// (`cache/`), which iOS backs up. They are only a cache: removed once.
    static func removeRustCache(_ paths: AppPaths) {
        let old = paths.dataDir.appendingPathComponent("cache", isDirectory: true)
        if FileManager.default.fileExists(atPath: old.path) {
            try? FileManager.default.removeItem(at: old)
            Log.note("removed the old picture cache from the data folder")
        }
    }
}
