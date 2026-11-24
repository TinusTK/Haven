import Foundation
import SQLite3
import Testing
@testable import Haven

/// The Rust build's storage tests (haven-core/src/storage.rs), ported.
struct StoreTests {
    @Test func changesWaitInOrderAndCancelOut() throws {
        let store = try Store.inMemory()
        func change(_ what: String, _ title: String) -> Change {
            Change(what: what, kind: "movie", titleID: title, videoID: title, createdAt: 1)
        }
        try store.queueChange(change("watched", "tt1"))
        try store.queueChange(change("watched", "tt1"))
        try store.queueChange(change("watchlist_add", "tt2"))
        try store.queueChange(change("watchlist_remove", "tt2"))
        let waiting = try store.queuedChanges()
        #expect(waiting.map(\.what) == ["watched", "watchlist_remove"])
        try store.removeChange(id: waiting[0].id)
        #expect(try store.queuedChanges().count == 1)
    }

    @Test func downloadsAreKeptAndSorted() throws {
        let store = try Store.inMemory()
        let base = Download(
            kind: "series", titleID: "tt1", videoID: "tt1:1:2", title: "Show", detail: "S1 E2",
            season: 1, episode: 2, poster: nil, url: "https://x/2.mkv", path: "/d/2.mkv", size: 0,
            received: 0, state: "waiting", addedAt: 1)
        let two = try store.addDownload(base)
        var one = base
        one.videoID = "tt1:1:1"
        one.episode = 1
        try store.addDownload(one)
        var film = base
        film.kind = "movie"
        film.title = "Film"
        film.season = nil
        film.episode = nil
        try store.addDownload(film)
        try store.updateDownload(id: two, size: 100, received: 50, state: "loading")
        let all = try store.downloads()
        #expect(all.map(\.episode) == [nil, 1, 2])
        #expect(all[2].size == 100 && all[2].received == 50 && all[2].state == "loading")
        try store.removeDownload(id: two)
        #expect(try store.downloads().count == 2)
    }

    @Test func newDatabaseIsAtTheLatestSchema() throws {
        let store = try Store.inMemory()
        #expect(try store.schemaVersion() == Store.migrations.count)
        #expect(Store.migrations.count == 9) // the Rust build's schema
    }

    @Test func valuesRoundTripAndOverwrite() throws {
        let store = try Store.inMemory()
        #expect(try store.get("n", as: Int.self) == nil)
        try store.put("n", 1)
        try store.put("n", 2)
        #expect(try store.get("n", as: Int.self) == 2)
        try store.remove("n")
        #expect(try store.get("n", as: Int.self) == nil)
    }

#if ADDONS
    @Test func addonsKeepInstallOrderAndUpdateInPlace() throws {
        let store = try Store.inMemory()
        try store.saveAddon(url: "https://a/manifest.json", manifest: "{\"v\":1}")
        try store.saveAddon(url: "https://b/manifest.json", manifest: "{}")
        try store.saveAddon(url: "https://a/manifest.json", manifest: "{\"v\":2}")
        let addons = try store.addons()
        #expect(addons.count == 2)
        #expect(addons[0].url == "https://a/manifest.json")
        #expect(addons[0].manifest == "{\"v\":2}")
        try store.removeAddon(url: "https://a/manifest.json")
        #expect(try store.addons()[0].url == "https://b/manifest.json")
    }
#endif

    private func watched(_ titleID: String, _ videoID: String, _ position: Double, _ finished: Bool, _ at: Int64) -> WatchProgress {
        WatchProgress(kind: "series", titleID: titleID, videoID: videoID, title: "Show \(titleID)", detail: videoID,
                 poster: nil, position: position, duration: 3000, finished: finished, updatedAt: at)
    }

    @Test func progressRoundTripsAndIsReplaced() throws {
        let store = try Store.inMemory()
        #expect(try store.progress(kind: "series", titleID: "a", videoID: "a:1:1") == nil)
        var first = watched("a", "a:1:1", 100, false, 1)
        first.poster = "https://example.org/p.jpg"
        try store.saveProgress(first)
        #expect(try store.progress(kind: "series", titleID: "a", videoID: "a:1:1") == first)

        // Saved again without a poster: the place moves, the poster stays.
        try store.saveProgress(watched("a", "a:1:1", 250, false, 2))
        let again = try #require(try store.progress(kind: "series", titleID: "a", videoID: "a:1:1"))
        #expect(again.position == 250)
        #expect(again.updatedAt == 2)
        #expect(again.poster == "https://example.org/p.jpg")
    }

    @Test func anEpisodeCanBeMarkedUnwatchedAndTheLastMarkIsSent() throws {
        let store = try Store.inMemory()
        try store.saveProgress(watched("a", "a:1:1", 0, true, 10))
        try store.saveProgress(watched("a", "a:1:2", 0, true, 11))
        try store.removeVideoProgress(kind: "series", titleID: "a", videoID: "a:1:1")
        #expect(try store.finishedVideos(kind: "series", titleID: "a") == ["a:1:2"])
        func change(_ what: String) -> Change {
            Change(what: what, kind: "series", titleID: "a", videoID: "a:1:1", createdAt: 1)
        }
        try store.queueChange(change("watched"))
        try store.queueChange(change("unwatched"))
        let queued = try store.queuedChanges()
        #expect(queued.count == 1)
        #expect(queued[0].what == "unwatched")
    }

    @Test func continueWatchingShowsEachTitleOnceByItsLatestEpisode() throws {
        let store = try Store.inMemory()
        try store.saveProgress(watched("a", "a:1:1", 2900, true, 10))
        try store.saveProgress(watched("a", "a:1:2", 400, false, 20))
        try store.saveProgress(watched("b", "b:1:1", 300, false, 30))
        // The latest episode of c was finished: nothing to carry on with.
        try store.saveProgress(watched("c", "c:1:1", 300, false, 5))
        try store.saveProgress(watched("c", "c:1:2", 2900, true, 40))

        #expect(try store.continueWatching(limit: 10).map(\.videoID) == ["b:1:1", "a:1:2"])
        #expect(try store.continueWatching(limit: 1).count == 1)
        // History keeps the finished one too.
        #expect(try store.history(limit: 10).map(\.videoID) == ["c:1:2", "b:1:1", "a:1:2"])

        try store.removeProgress(kind: "series", titleID: "b")
        #expect(try store.continueWatching(limit: 10).map(\.videoID) == ["a:1:2"])
    }

    @Test func watchedElsewhereNeverReplacesWhatHavenHas() throws {
        let store = try Store.inMemory()
        try store.saveProgress(watched("a", "a:1:1", 100, false, 5))
        #expect(try store.addWatchedElsewhere(watched("a", "a:1:1", 0, true, 9)) == false)
        #expect(try store.addWatchedElsewhere(watched("a", "a:1:2", 0, false, 9)) == true)
        #expect(try store.progress(kind: "series", titleID: "a", videoID: "a:1:1")?.finished == false)
        #expect(try store.progress(kind: "series", titleID: "a", videoID: "a:1:2")?.finished == true)
    }

    @Test func serversGetNumbersAndSigningInAgainKeepsTheNumber() throws {
        let store = try Store.inMemory()
        func server(_ address: String, _ token: String) -> StoredServer {
            StoredServer(kind: "jellyfin", name: "Home", address: address, userID: "u", token: token)
        }
        let first = try store.saveServer(server("http://a", "one"))
        let second = try store.saveServer(server("http://b", "two"))
        #expect(first != second)
        #expect(try store.saveServer(server("http://a", "three")) == first)
        let servers = try store.servers()
        #expect(servers.count == 2)
        #expect(servers[0].token == "three")
        #expect(!String(describing: servers[0]).contains("three"))
        #expect(!String(reflecting: servers[0]).contains("three"))
        try store.removeServer(id: first)
        #expect(try store.servers()[0].id == second)
    }

    @Test func tvSourcesKeepTheirOrderAndRefreshInPlace() throws {
        let store = try Store.inMemory()
        func source(_ url: String, _ playlist: String) -> TvSource {
            TvSource(url: url, name: "name", playlist: playlist)
        }
        try store.saveTvSource(source("http://a/list.m3u", "one"))
        try store.saveTvSource(source("http://b/list.m3u", "two"))
        try store.saveTvSource(source("http://a/list.m3u", "three"))
        let sources = try store.tvSources()
        #expect(sources.count == 2)
        #expect(sources[0].url == "http://a/list.m3u")
        #expect(sources[0].playlist == "three")
        try store.removeTvSource(url: "http://a/list.m3u")
        #expect(try store.tvSources()[0].url == "http://b/list.m3u")
    }

    @Test func watchlistIsNewestFirstAndSavingTwiceKeepsOneEntry() throws {
        let store = try Store.inMemory()
        func saved(_ id: String, _ title: String, _ poster: String?, _ at: Int64) -> Saved {
            Saved(kind: "movie", titleID: id, title: title, year: "2026", poster: poster, addedAt: at)
        }
        #expect(try !store.inWatchlist(kind: "movie", titleID: "a"))
        try store.addToWatchlist(saved("a", "First", "https://example.org/a.jpg", 1))
        try store.addToWatchlist(saved("b", "Second", nil, 2))
        // Saved again later, renamed and without a poster.
        try store.addToWatchlist(saved("a", "First (renamed)", nil, 9))
        #expect(try store.inWatchlist(kind: "movie", titleID: "a"))
        #expect(try !store.inWatchlist(kind: "series", titleID: "a"))

        let list = try store.watchlist()
        #expect(list.map(\.title) == ["Second", "First (renamed)"])
        #expect(list[1].poster == "https://example.org/a.jpg")
        #expect(list[1].addedAt == 1)

        try store.removeFromWatchlist(kind: "movie", titleID: "b")
        #expect(try store.watchlist().count == 1)
    }

    // MARK: Files made outside this build (as the Rust build left them)

    private func tempFile() -> URL {
        FileManager.default.temporaryDirectory
            .appendingPathComponent("haven-test-\(UUID().uuidString).sqlite3")
    }

    /// Runs SQL on a database file with plain SQLite, as another build would.
    private func raw(_ url: URL, _ sql: String) throws {
        var db: OpaquePointer?
        guard sqlite3_open(url.path, &db) == SQLITE_OK else { throw HavenError.database("open") }
        defer { sqlite3_close(db) }
        var message: UnsafeMutablePointer<CChar>?
        guard sqlite3_exec(db, sql, nil, nil, &message) == SQLITE_OK else {
            let text = message.map { String(cString: $0) } ?? "?"
            sqlite3_free(message)
            throw HavenError.database(text)
        }
    }

    @Test func anOlderDatabaseGainsTheAddonsTable() throws {
        let url = tempFile()
        defer { try? FileManager.default.removeItem(at: url) }
        try raw(url, Store.migrations[0] + "PRAGMA user_version = 1; INSERT INTO kv VALUES ('k', '\"v\"');")
        let store = try Store.open(url)
        #expect(try store.get("k", as: String.self) == "v")
        #if ADDONS
        #expect(try store.addons().isEmpty)
        #endif
        #expect(try store.schemaVersion() == Store.migrations.count)
    }

    @Test func reopeningKeepsDataAndDoesNotRerunMigrations() throws {
        let url = tempFile()
        defer { try? FileManager.default.removeItem(at: url) }
        try Store.open(url).put("k", "v")
        let again = try Store.open(url)
        #expect(try again.get("k", as: String.self) == "v")
    }

    @Test func refusesADatabaseFromANewerBuild() throws {
        let url = tempFile()
        defer { try? FileManager.default.removeItem(at: url) }
        try raw(url, "PRAGMA user_version = 999;")
        #expect(throws: HavenError.databaseTooNew(found: 999, known: Store.migrations.count)) {
            try Store.open(url)
        }
    }

    @Test func aHavenHubPairingIsForgotten() throws {
        let url = tempFile()
        defer { try? FileManager.default.removeItem(at: url) }
        let before = Store.migrations.count - 1
        try raw(url, Store.migrations[..<before].joined(separator: "\n") + """
            PRAGMA user_version = \(before);
            INSERT INTO servers (kind, name, address, user_id, token) VALUES
              ('hub', 'Den', 'http://den', 'u', 't'), ('plex', 'Plex', 'http://plex', 'u', 't');
            """)
        let store = try Store.open(url)
        #expect(try store.servers().map(\.kind) == ["plex"])
    }

    /// A database the Rust build wrote reads back the same here.
    @Test func readsRowsTheRustBuildWrote() throws {
        let url = tempFile()
        defer { try? FileManager.default.removeItem(at: url) }
        try raw(url, Store.migrations.joined(separator: "\n") + """
            PRAGMA user_version = \(Store.migrations.count);
            INSERT INTO kv VALUES ('settings', '{"theme":"light","last_page":3}');
            INSERT INTO progress VALUES ('movie', 'tt1', 'tt1', 'Film', '', NULL, 612.5, 6000.0, 0, 1764000000000);
            """)
        let store = try Store.open(url)
        #expect(try Settings.load(store) == Settings(theme: .light, lastPage: 3))
        let p = try #require(try store.progress(kind: "movie", titleID: "tt1", videoID: "tt1"))
        #expect(p.position == 612.5 && !p.finished && p.updatedAt == 1_764_000_000_000)
    }
}
