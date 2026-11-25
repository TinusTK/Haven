import Foundation
import Testing
@testable import Haven

/// The Rust build's downloads tests (`library/downloads.rs`), ported, and
/// Library › Local, Downloads and History.
struct DownloadRulesTests {
    private func sample(_ kind: String, _ season: Int?, _ episode: Int?, _ detail: String) -> Download {
        Download(id: 1, kind: kind, titleID: "tt1", videoID: "v", title: "Dune: Part Two", detail: detail, season: season,
                 episode: episode, poster: nil, url: "", path: "", size: 0, received: 0, state: "waiting", addedAt: 0)
    }

    @Test func phoneFilesSitStraightInTheFolder() {
        #expect(DownloadRules.fileName(sample("movie", nil, nil, ""), extension: "mkv") == "Dune Part Two.mkv")
        #expect(DownloadRules.fileName(sample("series", 1, 2, "S1 E2"), extension: "mp4") == "Dune Part Two - S01E02.mp4")
        #expect(DownloadRules.fileName(sample("series", 1, 2, "S1 E2 · The Gift / of Men"), extension: "mp4")
            == "Dune Part Two - S01E02 - The Gift of Men.mp4")
    }

    @Test func aMovedPhoneFolderIsFoundAgain() {
        let old = "/private/var/mobile/Containers/Data/Application/AAAA/Documents/Downloads/Movies/Dune.mkv"
        let root = URL(fileURLWithPath: "/var/mobile/Containers/Data/Application/BBBB/Documents/Downloads")
        #expect(DownloadRules.movedPath(old, root: root) == root.appendingPathComponent("Movies/Dune.mkv"))
        #expect(DownloadRules.movedPath("/Users/me/Movies/Haven/Movies/Dune.mkv", root: root) == nil)
        #expect(DownloadRules.currentPath(old, root: root).path == root.appendingPathComponent("Movies/Dune.mkv").path)
        #expect(DownloadRules.currentPath(root.path + "/Dune.mkv", root: root).path == root.path + "/Dune.mkv")
    }

    @Test func theFileTypeComesFromTheAddress() {
        #expect(DownloadRules.extensionOf("https://x/a/film.MP4?token=1") == "mp4")
        #expect(DownloadRules.extensionOf("https://x/stream/123") == "mkv")
        #expect(DownloadRules.safeName("  ..  ") == "Untitled")
        #expect(DownloadRules.safeName("A\u{7}B:C") == "A B C")
    }

    @Test func theListGoesFilmsThenSeriesByName() {
        var film = sample("movie", nil, nil, "")
        film.state = "done"
        film.size = 2_500_000_000
        var one = sample("series", 1, 1, "S1 E1 · One")
        one.id = 2
        one.title = "A Show"
        one.state = "loading"
        var two = one
        two.id = 3
        two.detail = "S1 E2 · Two"
        two.state = "paused"
        two.size = 1000
        two.received = 250
        var three = two
        three.id = 4
        three.state = "failed"
        let rows = DownloadRules.rows([film, one, two, three], live: [2: (size: 2_000_000, received: 500_000)])
        #expect(rows.map(\.section) == ["Films", "Series", "", ""])
        #expect(rows.map(\.group) == ["", "A Show", "", ""])
        #expect(rows.map(\.title) == ["Dune: Part Two", "S1 E1 · One", "S1 E2 · Two", "S1 E2 · Two"])
        #expect(rows.map(\.status) == ["2.5 GB", "25% · 500 KB of 2 MB", "Paused · 25%", "Stopped: could not fetch it. Try again."])
        #expect(rows[0].done && rows[1].running && rows[3].failed && rows[0].progress == 1)
    }

    @Test func localFindsVideosAFewFoldersDown() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("haven-videos-\(UUID().uuidString)")
        let deep = root.appendingPathComponent("a/b/c/d")
        try FileManager.default.createDirectory(at: deep, withIntermediateDirectories: true)
        for path in ["Film.mkv", "notes.txt", ".hidden.mp4", "a/Two.MP4", "a/b/c/Three.avi", "a/b/c/d/TooDeep.mkv"] {
            try Data("x".utf8).write(to: root.appendingPathComponent(path))
        }
        let found = DownloadRules.findVideos(in: root)
        #expect(Set(found.map(\.name)) == ["Film", "Two", "Three"])
        #expect(found.first?.detail.hasPrefix("Haven › Videos") == true)
        #expect(DownloadRules.findVideos(in: root.appendingPathComponent("nowhere")).isEmpty)
        #expect(DownloadRules.localStatus(0) == DownloadRules.noLocalVideos)
        #expect(DownloadRules.localStatus(1) == "1 video." && DownloadRules.localStatus(3) == "3 videos.")
        try? FileManager.default.removeItem(at: root)
    }

    @Test func historySaysWhatIsLeft() {
        func progress(_ detail: String, finished: Bool) -> WatchProgress {
            WatchProgress(kind: "movie", titleID: "t", videoID: "t", title: "T", detail: detail, poster: nil,
                          position: 600, duration: 4200, finished: finished, updatedAt: 0)
        }
        #expect(DownloadRules.historyNote(progress("S1 E2 · Two", finished: false)) == "S1 E2 · Two")
        #expect(DownloadRules.historyNote(progress("", finished: true)) == "Watched")
        #expect(DownloadRules.historyNote(progress("", finished: false)) == "1:00:00 left")
    }
}

@MainActor
struct DownloadsTests {
    private func downloads() throws -> (Downloads, Store, URL) {
        let store = try Store.inMemory()
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("haven-downloads-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let downloads = Downloads(store: store, root: root)
        // A stand-in for Apple's background service: tasks that never run.
        downloads.begin = { _, url, _ in URLSession.shared.downloadTask(with: url) }
        downloads.pauseTask = { _ in }
        return (downloads, store, root)
    }

    private func film(_ id: String, address: String) -> PlayRequest {
        PlayRequest(address: address, title: "Film \(id)", detail: "", kind: "movie", titleID: id, videoID: id, poster: nil, source: "S")
    }

    @Test func twoRunAtOnceAndTheRestWait() throws {
        let (downloads, store, root) = try downloads()
        var played: [PlayRequest] = []
        let note = downloads.download(film("tt1", address: "https://x/one.mp4"), season: nil, episode: nil) { played.append($0) }
        #expect(note == "Downloading to On My iPhone › Haven › Downloads. It is in your Library under Downloads.")
        #expect(downloads.savedState("https://x/one.mp4") == 1)
        #expect(downloads.download(film("tt1", address: "https://x/one.mp4"), season: nil, episode: nil) { _ in }
            == "Still downloading. Press play to stream it now.")
        #expect(downloads.download(film("tt1", address: "https://y/other.mkv"), season: nil, episode: nil) { _ in }
            == "That one is already in your downloads (Library).")
        _ = downloads.download(film("tt2", address: "https://x/two.mkv"), season: nil, episode: nil) { _ in }
        _ = downloads.download(film("tt3", address: "https://x/three"), season: nil, episode: nil) { _ in }
        let all = try store.downloads()
        #expect(all.map(\.state) == ["loading", "loading", "waiting"])
        #expect(all.map { URL(fileURLWithPath: $0.path).lastPathComponent } == ["Film tt1.mp4", "Film tt2.mkv", "Film tt3.mkv"])
        #expect(downloads.count == 3 && downloads.rows.first?.section == "Films")

        // Progress shows; a finished one plays from its file and the next starts.
        let first = all[0]
        downloads.progress(first.id, received: 500_000, size: 2_000_000)
        #expect(downloads.rows.first?.progress == 0.25)
        try Data("film".utf8).write(to: URL(fileURLWithPath: first.path))
        downloads.ended(first.id, saved: true, received: 4, size: 4, why: nil)
        #expect(try store.downloads().map(\.state) == ["done", "loading", "loading"])
        #expect(downloads.savedState("https://x/one.mp4") == 2)
        #expect(downloads.download(film("tt1", address: "https://x/one.mp4"), season: nil, episode: nil) { played.append($0) } == "")
        #expect(played.map(\.address) == [first.path] && played.first?.source == "Downloaded")

        // Paused, then carried on.
        let second = all[1]
        downloads.toggle(second.id)
        downloads.ended(second.id, saved: false, received: 10, size: 100, why: "cancelled")
        #expect(try store.downloads().first { $0.id == second.id }?.state == "paused")
        downloads.toggle(second.id)
        #expect(try store.downloads().first { $0.id == second.id }?.state == "loading")

        // A failure says so; deleting takes the file away too.
        downloads.ended(all[2].id, saved: false, received: 0, size: 0, why: "the server answered 404")
        #expect(downloads.rows.contains { $0.failed })
        downloads.remove(first.id)
        #expect(!FileManager.default.fileExists(atPath: first.path))
        #expect(try store.downloads().count == 2)
        try? FileManager.default.removeItem(at: root)
    }

    @Test func anEpisodeIsNamedAndListedUnderItsSeries() throws {
        let (downloads, store, root) = try downloads()
        let request = PlayRequest(address: "https://x/e.mkv", title: "A Show", detail: "S2 E3 · Three", kind: "series", titleID: "tt9",
                                  videoID: "tt9:2:3", poster: nil, source: "S")
        _ = downloads.download(request, season: 2, episode: 3) { _ in }
        let saved = try #require(try store.downloads().first)
        #expect(saved.kind == "series" && saved.season == 2 && saved.episode == 3)
        #expect(URL(fileURLWithPath: saved.path).lastPathComponent == "A Show - S02E03 - Three.mkv")
        #expect(downloads.rows.first?.section == "Series" && downloads.rows.first?.group == "A Show")
        try? FileManager.default.removeItem(at: root)
    }

    @Test func historyFollowsWhatWasWatched() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("haven-\(UUID().uuidString)")
        let store = try Store.inMemory()
        let library = Library(store: store, keys: Secrets(), http: HTTP(protocolClasses: [FixtureServer.self]), kept: Kept(dir: folder))
        try store.saveProgress(WatchProgress(kind: "movie", titleID: "tt5", videoID: "tt5", title: "Five", detail: "", poster: nil,
                                             position: 600, duration: 6000, finished: false, updatedAt: 1))
        library.reloadContinue()
        #expect(library.history.map(\.title) == ["Five"])
        library.removeContinue(kind: "movie", id: "tt5")
        #expect(library.history.isEmpty)
    }
}
