package net.haven.app.library

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.runBlocking
import net.haven.app.core.Secrets
import net.haven.app.core.Store
import net.haven.app.metadata.Tmdb
import net.haven.app.pictures.Kept
import net.haven.app.servers.MediaLibrary
import net.haven.app.servers.Server
import net.haven.app.servers.ServerKind
import net.haven.app.testing.FakeWeb
import net.haven.app.titles.MetaPreview
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.util.concurrent.Executors

/** Search (the Rust build's `run_search`) and Explore (`library/explore.rs`). */
class ExploreTest {
    private val thread = Executors.newSingleThreadExecutor().asCoroutineDispatcher()
    private val scopes = mutableListOf<CoroutineScope>()

    @After fun stop() = scopes.forEach { it.cancel() }

    private fun library(web: FakeWeb, key: String? = null, kept: Kept = Kept(Files.createTempDirectory("haven-kept").toFile())): Library {
        val store = Store.open(File.createTempFile("haven-test-", ".sqlite3").also { it.delete(); it.deleteOnExit() })
        val scope = CoroutineScope(SupervisorJob() + thread).also { scopes += it }
        return Library(store, Secrets(tmdbApiKey = key), web.http, kept, scope)
    }

    private fun <T> on(block: () -> T): T = runBlocking(thread) { block() }

    private fun waitFor(test: () -> Boolean) {
        repeat(100) { if (on(test)) return; Thread.sleep(30) }
    }

    /** A TMDB page of films numbered `from` through `to`. */
    private fun films(from: Int, to: Int) =
        """{"results": [${(from..to).joinToString(",") { """{"id": $it, "title": "Film $it", "poster_path": "/p$it.jpg"}""" }}]}"""

    private fun discover(key: String, page: Int, genre: Int? = null) =
        "${Tmdb.API}/discover/movie?sort_by=popularity.desc&page=$page" + (genre?.let { "&with_genres=$it" } ?: "") + "&api_key=$key"

    // Search

    @Test fun searchAsksEverySearchableCatalogueAndTmdb() {
        val library = library(FakeWeb(), key = "search-key")
        on { library.runSearch("  dune ") }
        // Cinemeta's two searchable catalogues, then TMDB's films and series.
        assertEquals(listOf("Films", "Series", "Films", "Series"), library.searchBoard.shelves.map { it.title })
        assertEquals(listOf("Cinemeta", "Cinemeta", "TMDB", "TMDB"), library.searchBoard.shelves.map { it.source })
        assertTrue("search results are not kept", library.searchBoard.shelves.all { it.keptName.isEmpty() })
        assertEquals("", library.searchNote)
        on { library.runSearch("   ") }
        assertTrue(library.searchBoard.shelves.isEmpty())
    }

    // Explore

    @Test fun sourcesAreListedUnderWhereTheyComeFrom() {
        val library = library(FakeWeb(), key = "rows-key")
        val explore = library.explore
        on { explore.rebuild() }
        assertEquals(ExploreSource.TmdbAll, explore.sources.first())
        assertEquals("TMDB and Cinemeta's four catalogues", 5, explore.sources.size)
        assertEquals(listOf("TMDB", "Cinemeta"), explore.rows.filter { it.header }.map { it.label })
        assertTrue(explore.source == 0 && explore.kinds == listOf("Films", "Series"))
        on { explore.findSource("series cinemeta") }
        assertTrue(explore.rows.first().label == "Cinemeta" && explore.rows.size == 3)
        on { explore.findSource("nothing like it") }
        assertTrue(explore.rows.isEmpty())
        // No TMDB key: TMDB is no longer a source, and the first one is chosen.
        on { library.useKeys(Secrets()) }
        assertEquals(4, explore.sources.size)
        assertTrue(explore.source == 0 && explore.kinds.isEmpty())
    }

    @Test fun tmdbPagesComeTwoAtATimeWithoutRepeats() {
        val key = "pages-key"
        val web = FakeWeb().apply {
            answer("${Tmdb.API}/genre/movie/list?api_key=$key", """{"genres": [{"id": 28, "name": "Action"}]}""")
            answer(discover(key, 1), films(1, 20))
            answer(discover(key, 2), films(21, 40))
            answer(discover(key, 3), films(39, 41))
        }
        val library = library(web, key)
        val explore = library.explore
        on { explore.rebuild() }
        waitFor { explore.genres.isNotEmpty() }
        assertEquals(listOf("All genres", "Action"), explore.genres)
        assertEquals("nothing is fetched before Explore is opened", 0, web.count(discover(key, 1)))
        on { explore.appeared() }
        waitFor { explore.items.size == 40 }
        assertTrue(explore.items.size == 40 && explore.more && explore.note == "")
        assertEquals("tmdb:movie:1", explore.items[0].id)
        on { explore.load() }
        waitFor { !explore.loading }
        // Page 3 repeats two films; page 4 fails but page 3 came.
        assertEquals(41, explore.items.size)
        assertFalse("fewer than two full pages: the end", explore.more)

        // The next start shows the first two pages at once, kept.
        val again = library(web, key, library.kept)
        on { again.explore.rebuild() }
        assertEquals(40, again.explore.items.size)

        // A genre that cannot be loaded says so.
        web.answer(discover(key, 1, 28), "{}", status = 500)
        on { explore.chooseGenre(1) }
        assertTrue(explore.genre == 0 && explore.items.isEmpty())
        waitFor { explore.note.isNotEmpty() }
        assertTrue(explore.note, explore.note.startsWith("Could not load this: "))
        assertFalse(explore.more)
    }

    @Test fun aRefreshKeepsTheTitlesUntilTheFreshOnesCome() {
        val key = "refresh-key"
        val web = FakeWeb().apply { answer(discover(key, 1), films(1, 20)) }
        val library = library(web, key)
        val explore = library.explore
        on {
            explore.rebuild()
            explore.appeared()
        }
        waitFor { explore.items.size == 20 }
        assertFalse(explore.more)
        web.answer(discover(key, 1), films(5, 24))
        runBlocking(thread) { explore.refresh() }
        assertTrue(explore.items.first().id == "tmdb:movie:5" && explore.items.size == 20)
        assertFalse(explore.refreshing)
    }

    @Test fun titlesAreAddedOnce() {
        fun title(id: String, name: String, year: String? = null) = MetaPreview(id, "movie", name, releaseInfo = year)
        val (list, added) = Explore.adding(
            listOf(title("tt2", "Two"), title("tt1", "One"), title("tt9", "One"), title("tt3", "Three", "2020"), title("tt4", "Three", "2021")),
            listOf(title("tt1", "One")),
        )
        assertEquals("the same id, or the same name and year, is a repeat", listOf("tt1", "tt2", "tt3", "tt4"), list.map { it.id })
        assertEquals(3, added)
        assertTrue(Explore.choices(emptyList()).isEmpty() && Explore.choices(listOf("Drama")) == listOf("All genres", "Drama"))
    }

    @Test fun aSourceIsLabelledByNameAndOrigin() {
        val server = Server(kind = ServerKind.Jellyfin, name = "Home", address = "http://192.168.1.2:8096", token = "t")
        assertEquals("Films and series  ·  TMDB", ExploreSource.TmdbAll.label)
        assertEquals("Films  ·  Home", ExploreSource.OnServer(server, MediaLibrary("1", "Films", false)).label)
    }
}
