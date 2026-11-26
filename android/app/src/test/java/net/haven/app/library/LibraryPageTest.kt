package net.haven.app.library

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.runBlocking
import net.haven.app.core.Download
import net.haven.app.core.Secrets
import net.haven.app.core.Store
import net.haven.app.core.WatchProgress
import net.haven.app.pictures.Kept
import net.haven.app.testing.FakeWeb
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.util.concurrent.Executors

/** The Rust build's download rules (`library/downloads.rs`), ported. */
class DownloadRulesTest {
    private fun sample(kind: String, season: Int?, episode: Int?, detail: String) =
        Download(1, kind, "tt1", "tt1", "Dune: Part Two", detail, season, episode, null, "https://x/f", "", 0, 0, "waiting", 0)

    @Test fun filmsAndEpisodesGoInTheirOwnFolders() {
        assertEquals("Movies/Dune Part Two.mkv", DownloadRules.relativePath(sample("movie", null, null, ""), "mkv"))
        assertEquals("Series/Dune Part Two/Season 1/Dune Part Two - S01E02.mp4", DownloadRules.relativePath(sample("series", 1, 2, "S1 E2"), "mp4"))
        assertEquals("Dune Part Two - S01E02 - The Gift of Men.mp4", DownloadRules.fileName(sample("series", 1, 2, "S1 E2 · The Gift / of Men"), "mp4"))
        assertEquals("Dune Part Two - S03.mkv", DownloadRules.fileName(sample("series", 3, null, ""), "mkv"))
    }

    @Test fun theFileTypeComesFromTheAddress() {
        assertEquals("mp4", DownloadRules.extensionOf("https://x/a/film.MP4?token=1"))
        assertEquals("mkv", DownloadRules.extensionOf("https://x/stream/123"))
        assertEquals("Untitled", DownloadRules.safeName("  ..  "))
    }

    @Test fun theFolderIsMoviesHavenWhenAllowed() {
        val movies = Files.createTempDirectory("Movies").toFile()
        val data = Files.createTempDirectory("data").toFile()
        assertEquals(File(movies, "Haven"), DownloadRules.root(movies, data))
        assertEquals(File(data, "Downloads"), DownloadRules.root(null, data))
        assertEquals("Movies › Haven on this device", DownloadRules.placeWords(File("/storage/emulated/0/Movies/Haven")))
        assertEquals("Haven's own folder on this device", DownloadRules.placeWords(File(data, "Downloads")))
    }

    @Test fun theListGoesFilmsThenSeriesByName() {
        val film = sample("movie", null, null, "").copy(state = "done", size = 2_500_000_000)
        val one = sample("series", 1, 1, "S1 E1 · One").copy(id = 2, title = "A Show", state = "loading")
        val two = one.copy(id = 3, detail = "S1 E2 · Two", state = "paused", size = 1000, received = 250)
        val three = two.copy(id = 4, state = "failed")
        val rows = DownloadRules.rows(listOf(film, one, two, three), mapOf(2L to (2_000_000L to 500_000L)))
        assertEquals(listOf("Films", "Series", "", ""), rows.map { it.section })
        assertEquals(listOf("", "A Show", "", ""), rows.map { it.group })
        assertEquals(listOf("Dune: Part Two", "S1 E1 · One", "S1 E2 · Two", "S1 E2 · Two"), rows.map { it.title })
        assertEquals(listOf("2.5 GB", "25% · 500 KB of 2 MB", "Paused · 25%", "Stopped: could not fetch it. Try again."), rows.map { it.status })
        assertTrue(rows[0].done && rows[1].running && rows[3].failed && rows[0].progress == 1.0)
    }

    @Test fun localAndHistorySayWhatThereIs() {
        assertEquals(DownloadRules.NO_LOCAL_VIDEOS, DownloadRules.localStatus(0))
        assertTrue(DownloadRules.localStatus(1) == "1 video." && DownloadRules.localStatus(3) == "3 videos.")
        assertEquals("Movies  ·  1.2 GB", FoundVideo("Film", "Movies", 1_288_490_189, 0, "content://x/1").detail)
        fun progress(detail: String, finished: Boolean) = WatchProgress("movie", "t", "t", "T", detail, null, 600.0, 4200.0, finished, 0)
        assertEquals("S1 E2 · Two", DownloadRules.historyNote(progress("S1 E2 · Two", false)))
        assertEquals("Watched", DownloadRules.historyNote(progress("", true)))
        assertEquals("1:00:00 left", DownloadRules.historyNote(progress("", false)))
    }
}

class DownloadsTest {
    private val thread = Executors.newSingleThreadExecutor().asCoroutineDispatcher()
    private val scopes = mutableListOf<CoroutineScope>()

    @After fun stop() = scopes.forEach { it.cancel() }

    private fun <T> on(block: () -> T): T = runBlocking(thread) { block() }

    private fun downloads(): Triple<Downloads, Store, File> {
        val store = Store.open(File.createTempFile("haven-test-", ".sqlite3").also { it.delete(); it.deleteOnExit() })
        val root = Files.createTempDirectory("haven-downloads").toFile()
        val scope = CoroutineScope(SupervisorJob() + thread).also { scopes += it }
        val downloads = Downloads(store, root, scope, FakeWeb().http)
        // A stand-in for fetching: jobs that never run.
        downloads.begin = { _, _, _ -> Job() }
        return Triple(downloads, store, root)
    }

    private fun film(id: String, address: String) = PlayRequest(address, "Film $id", "", "movie", id, id, null, "S")

    @Test fun twoRunAtOnceAndTheRestWait() {
        val (downloads, store, root) = downloads()
        val played = mutableListOf<PlayRequest>()
        on {
            val note = downloads.download(film("tt1", "https://x/one.mp4"), null, null) { played += it }
            assertEquals("Downloading to Haven's own folder on this device. It is in your Library under Downloads.", note)
            assertEquals(1, downloads.savedState("https://x/one.mp4"))
            assertEquals("Still downloading. Press play to stream it now.", downloads.download(film("tt1", "https://x/one.mp4"), null, null) {})
            assertEquals("That one is already in your downloads (Library).", downloads.download(film("tt1", "https://y/other.mkv"), null, null) {})
            downloads.download(film("tt2", "https://x/two.mkv"), null, null) {}
            downloads.download(film("tt3", "https://x/three"), null, null) {}
        }
        val all = store.downloads()
        assertEquals(listOf("loading", "loading", "waiting"), all.map { it.state })
        assertEquals(listOf("Movies/Film tt1.mp4", "Movies/Film tt2.mkv", "Movies/Film tt3.mkv"), all.map { File(it.path).relativeTo(root).path })
        assertTrue(downloads.count == 3 && downloads.rows.first().section == "Films")

        // Progress shows; a finished one plays from its file and the next starts.
        val first = all[0]
        on { downloads.progress(first.id, 500_000, 2_000_000) }
        assertEquals(0.25, downloads.rows.first().progress, 0.0)
        File(first.path).apply { parentFile?.mkdirs(); writeText("film") }
        on { downloads.ended(first.id, true, 4, 4, null) }
        assertEquals(listOf("done", "loading", "loading"), store.downloads().map { it.state })
        assertEquals(2, downloads.savedState("https://x/one.mp4"))
        assertEquals("", on { downloads.download(film("tt1", "https://x/one.mp4"), null, null) { played += it } })
        assertTrue(played.map { it.address } == listOf(first.path) && played.first().source == "Downloaded")

        // Paused, then carried on.
        val second = all[1]
        on {
            downloads.toggle(second.id)
            downloads.ended(second.id, false, 10, 100, "cancelled")
        }
        assertEquals("paused", store.downloads().first { it.id == second.id }.state)
        on { downloads.toggle(second.id) }
        assertEquals("loading", store.downloads().first { it.id == second.id }.state)

        // A failure says so; deleting takes the file away too.
        on { downloads.ended(all[2].id, false, 0, 0, "the server answered 404") }
        assertTrue(downloads.rows.any { it.failed })
        on { downloads.remove(first.id) }
        assertFalse(File(first.path).exists())
        assertEquals(2, store.downloads().size)
    }

    @Test fun aDownloadIsFetchedAndCarriesOnAfterAPause() {
        val body = ByteArray(300_000) { (it % 251).toByte() }
        val server = okhttp3.mockwebserver.MockWebServer()
        server.dispatcher = object : okhttp3.mockwebserver.Dispatcher() {
            override fun dispatch(request: okhttp3.mockwebserver.RecordedRequest): okhttp3.mockwebserver.MockResponse {
                val from = request.getHeader("Range")?.removePrefix("bytes=")?.removeSuffix("-")?.toIntOrNull()
                return if (from != null) {
                    okhttp3.mockwebserver.MockResponse().setResponseCode(206).setBody(okio.Buffer().write(body.copyOfRange(from, body.size)))
                } else {
                    okhttp3.mockwebserver.MockResponse().setBody(okio.Buffer().write(body))
                }
            }
        }
        server.start()
        val store = Store.open(File.createTempFile("haven-test-", ".sqlite3").also { it.delete(); it.deleteOnExit() })
        val root = Files.createTempDirectory("haven-downloads").toFile()
        val scope = CoroutineScope(SupervisorJob() + thread).also { scopes += it }
        val downloads = Downloads(store, root, scope, net.haven.app.core.Http(), newsOn = thread)
        val address = server.url("/film.mp4").toString()
        // Part of it is there already (paused before): only the rest is asked for.
        val path = File(root, "Movies/Film tt7.mp4")
        path.parentFile!!.mkdirs()
        File(path.parentFile, path.name + ".part").writeBytes(body.copyOfRange(0, 100_000))
        on { downloads.download(film("tt7", address), null, null) {} }
        repeat(200) { if (store.downloads().first().state == "loading") Thread.sleep(25) }
        val saved = store.downloads().first()
        assertEquals("done", saved.state)
        assertEquals(path.path, saved.path)
        assertTrue(path.readBytes().contentEquals(body))
        assertEquals("bytes=100000-", server.takeRequest().getHeader("Range"))
        server.shutdown()
    }

    @Test fun anEpisodeIsNamedAndListedUnderItsSeries() {
        val (downloads, store, _) = downloads()
        val request = PlayRequest("https://x/e.mkv", "A Show", "S2 E3 · Three", "series", "tt9", "tt9:2:3", null, "S")
        on { downloads.download(request, 2, 3) {} }
        val saved = store.downloads().first()
        assertTrue(saved.kind == "series" && saved.season == 2 && saved.episode == 3)
        assertTrue(saved.path.endsWith("Series/A Show/Season 2/A Show - S02E03 - Three.mkv"))
        assertTrue(downloads.rows.first().section == "Series" && downloads.rows.first().group == "A Show")
    }

    @Test fun historyFollowsWhatWasWatched() {
        val store = Store.open(File.createTempFile("haven-test-", ".sqlite3").also { it.delete(); it.deleteOnExit() })
        val scope = CoroutineScope(SupervisorJob() + thread).also { scopes += it }
        val library = Library(store, Secrets(), FakeWeb().http, Kept(Files.createTempDirectory("haven-kept").toFile()), scope)
        store.saveProgress(WatchProgress("movie", "tt5", "tt5", "Five", "", null, 600.0, 6000.0, false, 1))
        on { library.reloadContinue() }
        assertEquals(listOf("Five"), library.history.map { it.title })
        on { library.removeContinue("movie", "tt5") }
        assertTrue(library.history.isEmpty())
    }
}
