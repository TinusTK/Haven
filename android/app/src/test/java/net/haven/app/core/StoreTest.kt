package net.haven.app.core

import androidx.sqlite.driver.bundled.BundledSQLiteDriver
import androidx.sqlite.execSQL
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/** The Rust build's storage tests (haven-core/src/storage.rs), ported as the
 *  Swift build has them. */
class StoreTest {
    private fun change(what: String, title: String) = Change(what = what, kind = "movie", titleId = title, videoId = title, createdAt = 1)

    @Test
    fun changesWaitInOrderAndCancelOut() {
        val store = Store.inMemory()
        store.queueChange(change("watched", "tt1"))
        store.queueChange(change("watched", "tt1"))
        store.queueChange(change("watchlist_add", "tt2"))
        store.queueChange(change("watchlist_remove", "tt2"))
        val waiting = store.queuedChanges()
        assertEquals(listOf("watched", "watchlist_remove"), waiting.map { it.what })
        store.removeChange(waiting[0].id)
        assertEquals(1, store.queuedChanges().size)
    }

    @Test
    fun downloadsAreKeptAndSorted() {
        val store = Store.inMemory()
        val base = Download(
            kind = "series", titleId = "tt1", videoId = "tt1:1:2", title = "Show", detail = "S1 E2",
            season = 1, episode = 2, poster = null, url = "https://x/2.mkv", path = "/d/2.mkv", size = 0,
            received = 0, state = "waiting", addedAt = 1,
        )
        val two = store.addDownload(base)
        store.addDownload(base.copy(videoId = "tt1:1:1", episode = 1))
        store.addDownload(base.copy(kind = "movie", title = "Film", season = null, episode = null))
        store.updateDownload(two, size = 100, received = 50, state = "loading")
        val all = store.downloads()
        assertEquals(listOf(null, 1, 2), all.map { it.episode })
        assertTrue(all[2].size == 100L && all[2].received == 50L && all[2].state == "loading")
        store.removeDownload(two)
        assertEquals(2, store.downloads().size)
    }

    @Test
    fun newDatabaseIsAtTheLatestSchema() {
        val store = Store.inMemory()
        assertEquals(Store.migrations.size, store.schemaVersion())
        assertEquals(9, Store.migrations.size) // the Rust build's schema
    }

    @Test
    fun valuesRoundTripAndOverwrite() {
        val store = Store.inMemory()
        assertNull(store.get<Int>("n"))
        store.put("n", 1)
        store.put("n", 2)
        assertEquals(2, store.get<Int>("n"))
        store.remove("n")
        assertNull(store.get<Int>("n"))
    }

    @Test
    fun addonsKeepInstallOrderAndUpdateInPlace() {
        val store = Store.inMemory()
        store.saveAddon("https://a/manifest.json", "{\"v\":1}")
        store.saveAddon("https://b/manifest.json", "{}")
        store.saveAddon("https://a/manifest.json", "{\"v\":2}")
        val addons = store.addons()
        assertEquals(2, addons.size)
        assertEquals("https://a/manifest.json", addons[0].url)
        assertEquals("{\"v\":2}", addons[0].manifest)
        store.removeAddon("https://a/manifest.json")
        assertEquals("https://b/manifest.json", store.addons()[0].url)
    }

    private fun watched(titleId: String, videoId: String, position: Double, finished: Boolean, at: Long, poster: String? = null) =
        WatchProgress("series", titleId, videoId, "Show $titleId", videoId, poster, position, 3000.0, finished, at)

    @Test
    fun progressRoundTripsAndIsReplaced() {
        val store = Store.inMemory()
        assertNull(store.progress("series", "a", "a:1:1"))
        val first = watched("a", "a:1:1", 100.0, false, 1, poster = "https://example.org/p.jpg")
        store.saveProgress(first)
        assertEquals(first, store.progress("series", "a", "a:1:1"))
        // Saved again without a poster: the place moves, the poster stays.
        store.saveProgress(watched("a", "a:1:1", 250.0, false, 2))
        val again = store.progress("series", "a", "a:1:1")!!
        assertEquals(250.0, again.position, 0.0)
        assertEquals(2L, again.updatedAt)
        assertEquals("https://example.org/p.jpg", again.poster)
    }

    @Test
    fun anEpisodeCanBeMarkedUnwatchedAndTheLastMarkIsSent() {
        val store = Store.inMemory()
        store.saveProgress(watched("a", "a:1:1", 0.0, true, 10))
        store.saveProgress(watched("a", "a:1:2", 0.0, true, 11))
        store.removeVideoProgress("series", "a", "a:1:1")
        assertEquals(listOf("a:1:2"), store.finishedVideos("series", "a"))
        fun mark(what: String) = Change(what = what, kind = "series", titleId = "a", videoId = "a:1:1", createdAt = 1)
        store.queueChange(mark("watched"))
        store.queueChange(mark("unwatched"))
        val queued = store.queuedChanges()
        assertEquals(1, queued.size)
        assertEquals("unwatched", queued[0].what)
    }

    @Test
    fun continueWatchingShowsEachTitleOnceByItsLatestEpisode() {
        val store = Store.inMemory()
        store.saveProgress(watched("a", "a:1:1", 2900.0, true, 10))
        store.saveProgress(watched("a", "a:1:2", 400.0, false, 20))
        store.saveProgress(watched("b", "b:1:1", 300.0, false, 30))
        // The latest episode of c was finished: nothing to carry on with.
        store.saveProgress(watched("c", "c:1:1", 300.0, false, 5))
        store.saveProgress(watched("c", "c:1:2", 2900.0, true, 40))
        assertEquals(listOf("b:1:1", "a:1:2"), store.continueWatching(10).map { it.videoId })
        assertEquals(1, store.continueWatching(1).size)
        // History keeps the finished one too.
        assertEquals(listOf("c:1:2", "b:1:1", "a:1:2"), store.history(10).map { it.videoId })
        store.removeProgress("series", "b")
        assertEquals(listOf("a:1:2"), store.continueWatching(10).map { it.videoId })
    }

    @Test
    fun watchedElsewhereFinishesOnlyAnOlderPartialRecord() {
        val store = Store.inMemory()
        // Started here at 5, finished on the service at 9.
        store.saveProgress(watched("a", "a:1:1", 100.0, false, 5))
        assertTrue(store.addWatchedElsewhere(watched("a", "a:1:1", 0.0, true, 9)))
        assertTrue(store.progress("series", "a", "a:1:1")!!.finished)
        // Played again here at 50 (a rewatch): an older watch leaves it be.
        store.saveProgress(watched("a", "a:1:3", 100.0, false, 50))
        assertFalse(store.addWatchedElsewhere(watched("a", "a:1:3", 0.0, true, 9)))
        assertFalse(store.progress("series", "a", "a:1:3")!!.finished)
        assertTrue(store.addWatchedElsewhere(watched("a", "a:1:2", 0.0, false, 9)))
        assertTrue(store.progress("series", "a", "a:1:2")!!.finished)
        // Already finished: nothing changes.
        assertFalse(store.addWatchedElsewhere(watched("a", "a:1:2", 0.0, true, 99)))
    }

    @Test
    fun serversGetNumbersAndSigningInAgainKeepsTheNumber() {
        val store = Store.inMemory()
        fun server(address: String, token: String) = StoredServer(kind = "jellyfin", name = "Home", address = address, userId = "u", token = token)
        val first = store.saveServer(server("http://a", "one"))
        val second = store.saveServer(server("http://b", "two"))
        assertTrue(first != second)
        assertEquals(first, store.saveServer(server("http://a", "three")))
        val servers = store.servers()
        assertEquals(2, servers.size)
        assertEquals("three", servers[0].token)
        assertFalse(servers[0].toString().contains("three"))
        store.removeServer(first)
        assertEquals(second, store.servers()[0].id)
    }

    @Test
    fun tvSourcesKeepTheirOrderAndRefreshInPlace() {
        val store = Store.inMemory()
        store.saveTvSource(TvSource("http://a/list.m3u", "name", "one"))
        store.saveTvSource(TvSource("http://b/list.m3u", "name", "two"))
        store.saveTvSource(TvSource("http://a/list.m3u", "name", "three"))
        val sources = store.tvSources()
        assertEquals(2, sources.size)
        assertEquals("http://a/list.m3u", sources[0].url)
        assertEquals("three", sources[0].playlist)
        store.removeTvSource("http://a/list.m3u")
        assertEquals("http://b/list.m3u", store.tvSources()[0].url)
    }

    @Test
    fun watchlistIsNewestFirstAndSavingTwiceKeepsOneEntry() {
        val store = Store.inMemory()
        fun saved(id: String, title: String, poster: String?, at: Long) = Saved("movie", id, title, "2026", poster, at)
        assertFalse(store.inWatchlist("movie", "a"))
        store.addToWatchlist(saved("a", "First", "https://example.org/a.jpg", 1))
        store.addToWatchlist(saved("b", "Second", null, 2))
        // Saved again later, renamed and without a poster.
        store.addToWatchlist(saved("a", "First (renamed)", null, 9))
        assertTrue(store.inWatchlist("movie", "a"))
        assertFalse(store.inWatchlist("series", "a"))
        val list = store.watchlist()
        assertEquals(listOf("Second", "First (renamed)"), list.map { it.title })
        assertEquals("https://example.org/a.jpg", list[1].poster)
        assertEquals(1L, list[1].addedAt)
        store.removeFromWatchlist("movie", "b")
        assertEquals(1, store.watchlist().size)
    }

    // Files made outside this build (as the Rust build left them).

    private fun tempFile(): File = File.createTempFile("haven-test-", ".sqlite3").also { it.delete(); it.deleteOnExit() }

    /** Runs SQL on a database file with plain SQLite, as another build would. */
    private fun raw(file: File, sql: String) {
        BundledSQLiteDriver().open(file.path).use { db ->
            Store.statements(sql).forEach { db.execSQL(it) }
        }
    }

    @Test
    fun anOlderDatabaseGainsTheAddonsTable() {
        val file = tempFile()
        raw(file, Store.migrations[0] + "PRAGMA user_version = 1; INSERT INTO kv VALUES ('k', '\"v\"');")
        Store.open(file).use { store ->
            assertEquals("v", store.get<String>("k"))
            assertTrue(store.addons().isEmpty())
            assertEquals(Store.migrations.size, store.schemaVersion())
        }
    }

    @Test
    fun reopeningKeepsDataAndDoesNotRerunMigrations() {
        val file = tempFile()
        Store.open(file).use { it.put("k", "v") }
        Store.open(file).use { assertEquals("v", it.get<String>("k")) }
    }

    @Test
    fun refusesADatabaseFromANewerBuild() {
        val file = tempFile()
        raw(file, "PRAGMA user_version = 999;")
        val error = assertThrows(HavenError.DatabaseTooNew::class.java) { Store.open(file) }
        assertEquals(999, error.found)
        assertEquals(Store.migrations.size, error.known)
    }

    @Test
    fun aHavenHubPairingIsForgotten() {
        val file = tempFile()
        val before = Store.migrations.size - 1
        raw(
            file,
            Store.migrations.take(before).joinToString("\n") + """
            PRAGMA user_version = $before;
            INSERT INTO servers (kind, name, address, user_id, token) VALUES
              ('hub', 'Den', 'http://den', 'u', 't'), ('plex', 'Plex', 'http://plex', 'u', 't');
            """,
        )
        Store.open(file).use { assertEquals(listOf("plex"), it.servers().map { s -> s.kind }) }
    }

    /** A database the Rust build wrote reads back the same here. */
    @Test
    fun readsRowsTheRustBuildWrote() {
        val file = tempFile()
        raw(
            file,
            Store.migrations.joinToString("\n") + """
            PRAGMA user_version = ${Store.migrations.size};
            INSERT INTO kv VALUES ('settings', '{"theme":"light","last_page":3}');
            INSERT INTO progress VALUES ('movie', 'tt1', 'tt1', 'Film', '', NULL, 612.5, 6000.0, 0, 1764000000000);
            """,
        )
        Store.open(file).use { store ->
            assertEquals(Settings(Theme.Light, 3), Settings.load(store))
            val p = store.progress("movie", "tt1", "tt1")!!
            assertTrue(p.position == 612.5 && !p.finished && p.updatedAt == 1_764_000_000_000)
        }
    }
}
