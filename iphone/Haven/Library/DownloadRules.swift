import Foundation

/// One line of the Library's Downloads list.
struct DownloadRow: Equatable, Sendable, Identifiable {
    var id: Int64
    /// "Films" or "Series" on the first row of its section; empty after.
    var section: String
    /// A series' name on its first episode; empty otherwise.
    var group: String
    var title: String
    var status: String
    /// 0 to 1.
    var progress: Double
    var done: Bool
    var running: Bool
    var failed: Bool
    /// The episode's picture (a film's backdrop), kept with the download.
    var picture: URL? = nil
}

/// Where downloads and videos go, and how they are named and listed. The
/// Rust build's `library/downloads.rs` and `library/local.rs` rules.
enum DownloadRules {
    /// Downloads fetched at the same time; the rest wait their turn.
    static let atOnce = 2
    /// How often a download's progress reaches the screen and the database.
    static let reportEvery: TimeInterval = 0.7
    /// The most videos Local lists.
    static let mostVideos = 1000
    static let videoKinds: Set<String> = ["mp4", "m4v", "mkv", "mov", "avi", "webm", "ts", "m2ts", "wmv", "flv", "mpg", "mpeg", "3gp"]

    static let placeWords = "On My iPhone › Haven › Downloads"
    static let localPlace = "Videos in Files › On My iPhone › Haven › Videos"
    static let nothingDownloaded = "Nothing downloaded yet. On a film's or an episode's page, press the download button beside a place to watch: it is saved to this iPhone and shows here, sorted into films and series."
    static let noLocalVideos = "No videos here yet. Move videos into Files › On My iPhone › Haven › Videos and they show up here."

    /// Files › On My iPhone › Haven: the app's Documents folder.
    static var documents: URL {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first ?? FileManager.default.temporaryDirectory
    }

    /// Files › On My iPhone › Haven › Downloads.
    static var downloadsFolder: URL { documents.appendingPathComponent("Downloads", isDirectory: true) }

    /// Files › On My iPhone › Haven › Videos, which Library › Local lists.
    static var videosFolder: URL { documents.appendingPathComponent("Videos", isDirectory: true) }

    /// A name that is safe as a file name everywhere.
    static func safeName(_ name: String) -> String {
        let cleaned = String(name.map { $0.unicodeScalars.contains(where: { $0.properties.generalCategory == .control }) || "<>:\"/\\|?*".contains($0) ? " " : $0 })
        let joined = cleaned.split(whereSeparator: \.isWhitespace).joined(separator: " ")
        let trimmed = joined.trimmingCharacters(in: CharacterSet(charactersIn: ". "))
        return trimmed.isEmpty ? "Untitled" : String(trimmed.prefix(120))
    }

    /// The file type, from the address (`mkv`, `mp4`…), else `mkv`.
    static func extensionOf(_ address: String) -> String {
        let path = (address.split(whereSeparator: { $0 == "?" || $0 == "#" }).first.map(String.init) ?? "").lowercased()
        return ["mkv", "mp4", "m4v", "avi", "mov", "webm", "ts", "wmv", "mpg"].first { path.hasSuffix(".\($0)") } ?? "mkv"
    }

    /// The file's name: `Dune.mkv`, or `Show - S01E02 - Its name.mkv`. On an
    /// iPhone every file sits straight in the Downloads folder.
    static func fileName(_ d: Download, extension ext: String) -> String {
        let show = safeName(d.title)
        guard d.kind != "movie", let season = d.season else { return "\(show).\(ext)" }
        let number = d.episode.map { String(format: "S%02dE%02d", season, $0) } ?? String(format: "S%02d", season)
        // "S1 E2 · Its name" gives the episode's name after the dot.
        if let mark = d.detail.range(of: " · ") {
            return "\(show) - \(number) - \(safeName(String(d.detail[mark.upperBound...]))).\(ext)"
        }
        return "\(show) - \(number).\(ext)"
    }

    /// A saved path under `root`. An iPhone app's folder moves with every
    /// update (its path holds a new number), so a path saved by an older
    /// version is looked for again under today's Downloads folder.
    static func currentPath(_ saved: String, root: URL) -> URL {
        if saved.hasPrefix(root.path) { return URL(fileURLWithPath: saved) }
        return movedPath(saved, root: root) ?? URL(fileURLWithPath: saved)
    }

    /// The part of `saved` after its `Documents/Downloads/`, under `root`.
    static func movedPath(_ saved: String, root: URL) -> URL? {
        guard let mark = saved.range(of: "/Documents/Downloads/") else { return nil }
        return root.appendingPathComponent(String(saved[mark.upperBound...]))
    }

    static func sizeWords(_ bytes: Int64) -> String {
        let bytes = Double(max(bytes, 0))
        if bytes >= 1e9 { return String(format: "%.1f GB", bytes / 1e9) }
        if bytes >= 1e6 { return String(format: "%.0f MB", bytes / 1e6) }
        return String(format: "%.0f KB", max(bytes / 1e3, 1))
    }

    /// The Downloads list: films, then series, each series under its name
    /// with its episodes in order (the order the database gives).
    /// Where downloads' pictures are kept (Haven's own data folder, not
    /// the Downloads folder people see in Files), one file per download.
    static var picturesFolder: URL {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? FileManager.default.temporaryDirectory
        return support.appendingPathComponent("Haven/download-pictures", isDirectory: true)
    }

    static func pictureFile(_ id: Int64, in folder: URL = picturesFolder) -> URL {
        folder.appendingPathComponent("\(id).img")
    }

    static func rows(_ all: [Download], live: [Int64: (size: Int64, received: Int64)],
                     picture: (Int64) -> URL? = { _ in nil }) -> [DownloadRow] {
        var rows: [DownloadRow] = []
        var lastSection = ""
        var lastShow = ""
        for d in all {
            let film = d.kind == "movie" || d.season == nil
            let section = film ? "Films" : "Series"
            let (size, received) = live[d.id] ?? (d.size, d.received)
            let progress = d.state == "done" ? 1 : size > 0 ? Double(received) / Double(size) : 0
            let status = switch d.state {
            case "done": sizeWords(max(size, received))
            case "paused": "Paused · \(Int((progress * 100).rounded()))%"
            case "failed": "Stopped: could not fetch it. Try again."
            case "waiting": "Waiting…"
            default: size > 0
                ? "\(Int((progress * 100).rounded()))% · \(sizeWords(received)) of \(sizeWords(size))"
                : "\(sizeWords(received)) so far"
            }
            rows.append(DownloadRow(id: d.id, section: section != lastSection ? section : "",
                                    group: !film && d.title != lastShow ? d.title : "",
                                    title: film || d.detail.isEmpty ? d.title : d.detail, status: status,
                                    progress: progress, done: d.state == "done", running: d.state == "loading",
                                    failed: d.state == "failed", picture: picture(d.id)))
            lastSection = section
            if !film { lastShow = d.title }
        }
        return rows
    }

    // MARK: Local

    /// A video found in Haven › Videos.
    struct Found: Equatable, Sendable, Identifiable {
        var name: String
        var place: String
        var size: Int64
        var modified: Date
        var path: String
        var id: String { path }

        /// "Haven › Videos  ·  1.2 GB"
        var detail: String {
            [place, size > 0 ? localSize(size) : ""].filter { !$0.isEmpty }.joined(separator: "  ·  ")
        }
    }

    static func localSize(_ bytes: Int64) -> String {
        let mb = 1024.0 * 1024
        let bytes = Double(bytes)
        return bytes >= 1024 * mb ? String(format: "%.1f GB", bytes / (1024 * mb)) : String(format: "%.0f MB", bytes / mb)
    }

    /// Every video in `root`, 3 folders deep, newest first (at most 1000).
    static func findVideos(in root: URL, place: String = "Haven › Videos") -> [Found] {
        var found: [Found] = []
        scan(root, place: place, into: &found, depth: 3)
        return found.sorted { $0.modified > $1.modified }
    }

    private static func scan(_ folder: URL, place: String, into found: inout [Found], depth: Int) {
        let keys: [URLResourceKey] = [.isDirectoryKey, .fileSizeKey, .contentModificationDateKey]
        guard let entries = try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: keys) else { return }
        for entry in entries.sorted(by: { $0.lastPathComponent < $1.lastPathComponent }) {
            guard found.count < mostVideos else { return }
            guard !entry.lastPathComponent.hasPrefix("."), let values = try? entry.resourceValues(forKeys: Set(keys)) else { continue }
            if values.isDirectory == true {
                if depth > 0 { scan(entry, place: place, into: &found, depth: depth - 1) }
            } else if videoKinds.contains(entry.pathExtension.lowercased()) {
                found.append(Found(name: entry.deletingPathExtension().lastPathComponent, place: place,
                                   size: Int64(values.fileSize ?? 0), modified: values.contentModificationDate ?? .distantPast,
                                   path: entry.path))
            }
        }
    }

    /// "1 video.", "12 videos.", or how to add some.
    static func localStatus(_ count: Int) -> String {
        switch count {
        case 0: noLocalVideos
        case 1: "1 video."
        default: "\(count) videos."
        }
    }

    // MARK: History

    /// The line under a History poster: the episode, "Watched", or how much is left.
    static func historyNote(_ progress: WatchProgress) -> String {
        if !progress.detail.isEmpty { return progress.detail }
        if progress.finished { return "Watched" }
        return "\(clockText(progress.duration - progress.position)) left"
    }
}
