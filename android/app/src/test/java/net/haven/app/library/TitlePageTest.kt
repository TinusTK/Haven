package net.haven.app.library

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.runBlocking
import net.haven.app.core.Secrets
import net.haven.app.core.Store
import net.haven.app.core.WatchProgress
import net.haven.app.metadata.WhereToWatch
import net.haven.app.pictures.Kept
import net.haven.app.testing.FakeWeb
import net.haven.app.titles.Cinemeta
import net.haven.app.titles.MediaStream
import net.haven.app.titles.MetaDetail
import net.haven.app.titles.MetaPreview
import net.haven.app.titles.ResourceUrl
import net.haven.app.titles.Video
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.util.concurrent.Executors

/** The Rust build's title page tests (`library/sources.rs`, `marks.rs`), ported. */
class TitleRulesTest {
    private fun video(id: String, season: Int?, episode: Int?, released: String? = null, title: String? = "Pilot") =
        Video(id, title, season, episode, released)

    @Test fun sizesAreFoundInTheDetails() {
        val line = "Movie.2024.2160p 💾 12.3 GB 👤 50"
        val (size, range) = TitleRules.sizeIn(line)!!
        assertEquals("12.3 GB", size)
        assertEquals("Movie.2024.2160p 👤 50", TitleRules.tidy(line.removeRange(range)))
        assertEquals("850 MB", TitleRules.sizeIn("1080p · 850MB · WEB")?.first)
        assertEquals("1.4 GB", TitleRules.sizeIn("x 1,4 GiB")?.first)
        assertNull(TitleRules.sizeIn("H.264 5.1 Atmos 2160p"))
        assertNull(TitleRules.sizeIn("Show.S01E02.MBC"))
    }

    @Test fun howManyShareIsFoundInTheDetails() {
        val line = "BluRay REMUX 👥 418 🔍 Somewhere"
        val (count, range) = TitleRules.peersIn(line)!!
        assertEquals("418", count)
        assertEquals("BluRay REMUX 🔍 Somewhere", TitleRules.tidy(line.removeRange(range)))
        assertEquals("1204", TitleRules.peersIn("x 👤 1,204 y")?.first)
        assertNull(TitleRules.peersIn("1080p WEB-DL"))
    }

    @Test fun byteCountsReadWell() {
        assertEquals("12.3 GB", TitleRules.sizeText(13_207_024_435))
        assertEquals("850 MB", TitleRules.sizeText(891_289_600))
    }

    @Test fun anEpisodeIsDescribedAsPlayingItRecords() {
        assertEquals("S1 E2 · Pilot", TitleRules.episodeDetail(video("x:1:2", 1, 2)))
        assertEquals("E2", TitleRules.episodeDetail(video("x:2", null, 2, null, null)))
        val row = TitleRules.episodeRow(video("x:1:3", 1, 3, "2019-01-27T05:00:00.000Z", null))
        assertTrue(row.number == "E3" && row.title == "Episode 3" && row.date == "2019-01-27")
        assertEquals("Untitled", TitleRules.episodeRow(video("x", null, null, null, null)).title)
    }

    @Test fun onlyAiredEpisodesCountAsOut() {
        assertTrue(TitleRules.aired(video("a", 1, 1, "2020-01-01T00:00:00.000Z"), "2026-11-22"))
        assertFalse(TitleRules.aired(video("a", 1, 1, "2027-01-01"), "2026-11-22"))
        assertTrue(TitleRules.aired(video("a", 1, 1, null), "2026-11-22"))
    }

    @Test fun aStreamRowSplitsItsDetails() {
        val stream = MediaStream(name = "1080p\nWEB", description = "Some.Film.2024.1080p.WEB-DL · 💾 2.1 GB · 👥 12 · English · 5.1 · H.264 · from a long list of facts", url = "https://cdn.example/film.mkv")
        val row = TitleRules.streamRow(1, "Home (Jellyfin)", stream)!!
        assertEquals("1080p WEB", row.name)
        assertTrue(row.size == "2.1 GB" && row.peers == "12")
        assertEquals("Some.Film.2024.1080p.WEB-DL · English · 5.1", row.detail)
        assertEquals("H.264 · from a long list of facts", row.detail2)
        assertTrue(row.kind == "Direct" && row.playable && row.group == "Home (Jellyfin)")
        // A size hint wins over the text; a page and a local file are not played.
        val hinted = TitleRules.streamRow(2, "A", MediaStream(description = "3 GB", url = "https://x/y", videoSize = 891_289_600))!!
        assertTrue(hinted.size == "850 MB" && hinted.detail.isEmpty())
        assertEquals("Web page", TitleRules.streamRow(3, "A", MediaStream(externalUrl = "https://x"))?.kind)
        assertEquals(false, TitleRules.streamRow(4, "A", MediaStream(url = "file:///etc/hosts"))?.playable)
        assertEquals("Peer to peer", TitleRules.streamRow(5, "A", MediaStream(infoHash = "abc"))?.kind)
        assertNull(TitleRules.streamRow(6, "A", MediaStream(name = "Nothing")))
        assertEquals("Gathered", TitleRules.streamRow(7, "A", MediaStream(url = "https://x", provider = "Gathered"))?.group)
    }

    private fun row(id: Int, group: String) = StreamRow(id, group, "$id", "", "", "", "", "Direct", group = group)

    @Test fun eachProviderShowsTenAtATimeUnderItsHeading() {
        val rows = (0 until 12).map { row(it, "A") } + row(12, "B")
        val marked = TitleRules.marked(rows, emptyMap())
        assertTrue(marked[0].heading == "A  ·  12" && marked[1].heading.isEmpty())
        assertTrue(marked[9].more == 2 && marked[9].shown && !marked[10].shown)
        assertTrue(marked[12].heading == "B  ·  1" && marked[12].more == 0)
        val more = TitleRules.marked(rows, mapOf("A" to 20))
        assertTrue(more.count { it.shown } == 13 && more.all { it.more == 0 })
        assertEquals(listOf("A" to 12, "B" to 1), TitleRules.groups(rows))
    }

    @Test fun serversStayAboveAndProvidersStayTogether() {
        val rows = listOf(row(0, "Server"), row(0, "Add-on A"), row(0, "Add-on B"))
        assertEquals(1, TitleRules.place(row(0, "Server"), rows, 1, true))
        assertEquals(1, TitleRules.place(row(0, "Other server"), rows, 1, true))
        assertEquals(2, TitleRules.place(row(0, "Add-on A"), rows, 1, false))
        assertEquals(3, TitleRules.place(row(0, "Add-on C"), rows, 1, false))
    }

    @Test fun theNextEpisodeSkipsSpecials() {
        val meta = MetaDetail("tt1", "series", "Show", videos = listOf(video("tt1:1:2", 1, 2), video("tt1:0:1", 0, 1), video("tt1:2:1", 2, 1), video("tt1:1:1", 1, 1)))
        assertEquals("tt1:1:2", TitleRules.nextVideo(meta, "tt1:1:1")?.id)
        assertEquals("tt1:2:1", TitleRules.nextVideo(meta, "tt1:1:2")?.id)
        assertNull(TitleRules.nextVideo(meta, "tt1:2:1"))
    }

    @Test fun addOnsAreAskedByTheImdbForm() {
        val meta = MetaDetail("srv:1:abc", "series", "Show", imdbId = "tt0944947", releaseInfo = "2011", videos = listOf(video("srv:1:ep", 3, 4)))
        val wanted = TitleRules.wanted("series", "srv:1:ep", "srv:1:abc", meta, "")
        assertTrue(wanted.imdb == "tt0944947" && wanted.year == "2011" && wanted.episode == (3 to 4))
        assertEquals("tt0944947:3:4", TitleRules.addonId("srv:1:ep", wanted))
        assertEquals("an add-on's own id", "tt0944947:3:4", TitleRules.addonId("tt0944947:3:4", wanted))
        val film = TitleRules.wanted("movie", "tmdb:movie:603", "tmdb:movie:603", null, "The Matrix")
        assertTrue(film.tmdb == "603" && film.imdb == null && film.name == "The Matrix")
        assertNull(TitleRules.addonId("tmdb:movie:603", film))
        assertEquals("tt0944947", TitleRules.wanted("series", "tt0944947:1:1", "x", null, "S").imdb)
    }

    @Test fun anImdbIdIsFoundByNameAndYear() {
        val found = listOf(MetaPreview("tt1", "movie", "Dune", releaseInfo = "1984"), MetaPreview("kitsu:2", "movie", "Dune: Part Two", releaseInfo = "2024"), MetaPreview("tt1160419", "movie", "Dune", releaseInfo = "2021"))
        assertEquals("tt1160419", TitleRules.imdbMatch(found, "DUNE", "2021"))
        assertEquals("tt1", TitleRules.imdbMatch(found, "Dune", null))
        assertNull("not an IMDb id", TitleRules.imdbMatch(found, "Dune Part Two", "2024"))
        assertNull(TitleRules.imdbMatch(found, "  ", null))
    }
}

class TitlePageTest {
    private val thread = Executors.newSingleThreadExecutor().asCoroutineDispatcher()
    private val scopes = mutableListOf<CoroutineScope>()

    @After fun stop() = scopes.forEach { it.cancel() }

    private fun library(web: FakeWeb): Library {
        val store = Store.open(File.createTempFile("haven-test-", ".sqlite3").also { it.delete(); it.deleteOnExit() })
        val scope = CoroutineScope(SupervisorJob() + thread).also { scopes += it }
        return Library(store, Secrets(), web.http, Kept(Files.createTempDirectory("haven-kept").toFile()), scope)
    }

    private fun <T> on(block: () -> T): T = runBlocking(thread) { block() }

    private fun waitFor(test: () -> Boolean) {
        repeat(100) { if (on(test)) return; Thread.sleep(30) }
    }

    private val series = """
        {"meta": {"id": "tt7700001", "type": "series", "name": "A Show", "releaseInfo": "2020–", "imdbRating": "8.1",
                  "genres": ["Drama", "Crime", "Mystery", "Thriller"], "description": "  Things happen. ", "cast": ["A", "B"],
                  "videos": [{"id": "tt7700001:1:1", "title": "One", "season": 1, "episode": 1, "released": "2020-01-01T00:00:00.000Z"},
                             {"id": "tt7700001:1:2", "title": "Two", "season": 1, "episode": 2, "released": "2020-01-08T00:00:00.000Z"},
                             {"id": "tt7700001:1:3", "title": "Three", "season": 1, "episode": 3, "released": "2099-01-01T00:00:00.000Z"},
                             {"id": "tt7700001:2:1", "title": "Again", "season": 2, "episode": 1, "released": "2021-01-01T00:00:00.000Z"},
                             {"id": "tt7700001:0:1", "title": "Extra", "season": 0, "episode": 1}]}}
        """

    private fun web() = FakeWeb().apply { answer(ResourceUrl.make(Cinemeta.URL, "meta", "series", "tt7700001"), series) }

    @Test fun aSeriesOpensOnItsSeasonsAndMarksAiredEpisodes() {
        val library = library(web())
        val page = library.title
        on { page.open(Opening("tt7700001", "series", "A Show")) }
        assertEquals("A Show", page.title)
        waitFor { !page.loading }
        assertEquals("2020–  ·  ★ 8.1  ·  Drama, Crime, Mystery", page.facts)
        assertEquals("Things happen.", page.description)
        assertEquals("A, B", page.cast)
        assertEquals("specials last", listOf(1, 2, 0), page.seasons)
        assertTrue(page.season == 1 && page.episodes.map { it.title } == listOf("One", "Two", "Three"))
        assertTrue(page.isSeries && !page.sourcesShown)
        assertEquals(TitleRules.CHOOSE_EPISODE, page.sources.note)
        assertEquals(WhereToWatch.NO_KEY_NOTE, page.offersNote)

        on { page.markSeason(true) }
        assertEquals("the one not out yet stays unwatched", listOf(true, true, false), page.episodes.map { it.watched })
        assertFalse(page.seasonAllWatched)
        on { page.markEpisodes(listOf("tt7700001:1:3"), true) }
        assertTrue(page.seasonAllWatched)
        assertTrue(library.marks > 0)
        val saved = library.store.progress("series", "tt7700001", "tt7700001:1:2")
        assertTrue(saved?.finished == true && saved.detail == "S1 E2 · Two" && saved.title == "A Show")
        on { page.markSeason(false) }
        assertTrue(page.episodes.none { it.watched })

        on { page.filterEpisodes("tw") }
        assertEquals(listOf("Two"), page.episodes.map { it.title })
        on { page.filterEpisodes("") }
        on { page.openEpisode("tt7700001:1:2") }
        assertTrue(page.episodeOpen && page.panelOpen && page.sourcesShown)
        assertEquals("S1 E2 · Two", page.episodeLabel)
        assertEquals(SourcesPanel.Listed("tt7700001:1:2", "S1 E2 · Two"), page.sources.listed)
        assertEquals("no server, add-on or account to ask", TitleRules.nothingCanPlay, page.sources.note)
        assertEquals("tt7700001:1:2", page.episodes.firstOrNull { it.selected }?.id)

        // Opened again: the kept details show at once.
        on {
            page.close()
            page.open(Opening("tt7700001", "series", "A Show"))
        }
        assertTrue(page.seasons == listOf(1, 2, 0) && page.facts.startsWith("2020–"))
    }

    @Test fun aContinueWatchingCardOpensItsEpisode() {
        val library = library(web())
        val page = library.title
        on { page.open(Opening("tt7700001", "series", "A Show", video = "tt7700001:2:1")) }
        waitFor { !page.loading }
        assertTrue(page.season == 2 && page.episodeOpen && page.episodeLabel == "S2 E1 · Again")
    }

    @Test fun aFilmIsMarkedAndSavedToTheWatchlist() {
        val web = FakeWeb().apply {
            answer(ResourceUrl.make(Cinemeta.URL, "meta", "movie", "tt7700002"),
                """{"meta": {"id": "tt7700002", "type": "movie", "name": "A Film", "releaseInfo": "2024", "runtime": "101 min", "poster": "https://p/f.jpg"}}""")
        }
        val library = library(web)
        val queued = mutableListOf<String>()
        library.queueSync = { what, _, _, _ -> queued += what }
        val page = library.title
        on { page.open(Opening("tt7700002", "movie", "A Film")) }
        waitFor { !page.loading }
        assertTrue(page.film && !page.isSeries && page.sourcesShown)
        assertEquals("2024  ·  101 min", page.facts)
        assertEquals("https://p/f.jpg", page.poster)

        on { page.markFilm(true) }
        assertTrue(page.watched && library.filmWatched("movie", "tt7700002"))
        on { page.showFilmSources() }
        assertEquals("You have watched this.", page.resumeNote)
        on { page.markFilm(false) }
        assertTrue(!page.watched && !library.filmWatched("movie", "tt7700002"))
        assertEquals(listOf("watched", "unwatched"), queued)

        on { page.toggleWatchlist() }
        assertTrue(page.saved && library.watchlist.map { it.title } == listOf("A Film"))
        assertEquals("2024", library.watchlist.first().year)
        on { page.toggleWatchlist() }
        assertTrue(!page.saved && library.watchlist.isEmpty())
        assertEquals(listOf("watchlist_add", "watchlist_remove"), queued.takeLast(2))

        // A film part watched says where it resumes.
        library.store.saveProgress(WatchProgress("movie", "tt7700002", "tt7700002", "A Film", "", null, 600.0, 6000.0, false, 1))
        on { page.showResumeNote() }
        assertTrue(page.resumeNote, page.resumeNote.startsWith("Resumes at "))
        // Nothing plays until Haven's player comes.
        on { page.play(StreamRow(0, "S", "n", "", "", "", "", "Direct", "https://x/f.mp4", "S")) }
        assertTrue(page.playNote.isNotEmpty())
        // With a player, the request carries the film.
        var asked: PlayRequest? = null
        library.play = { asked = it }
        on { page.play(StreamRow(0, "S", "n", "", "", "", "", "Direct", "https://x/f.mp4", "S")) }
        assertNotNull(asked)
        assertTrue(asked!!.titleId == "tt7700002" && asked!!.videoId == "tt7700002" && asked!!.source == "S")
    }

    @Test fun aTitleNothingKnowsSaysSo() {
        val library = library(FakeWeb())
        val page = library.title
        on { page.open(Opening("tmdb:movie:1", "movie", "Gone")) }
        assertTrue(page.error == TitleRules.NO_TMDB_KEY && !page.loading)
        on { page.open(Opening("kitsu:1", "series", "Gone")) }
        assertEquals(TitleRules.noDetails, page.error)
    }
}
