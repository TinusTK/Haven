package net.haven.app.library

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import net.haven.app.core.Saved
import net.haven.app.core.Secrets
import net.haven.app.core.Store
import net.haven.app.core.WatchProgress
import net.haven.app.metadata.Omdb
import net.haven.app.pictures.Kept
import net.haven.app.testing.FakeWeb
import net.haven.app.titles.Catalog
import net.haven.app.titles.Cinemeta
import net.haven.app.titles.MetaDetail
import net.haven.app.titles.ResourceUrl
import net.haven.app.titles.Titles
import net.haven.app.titles.Video
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.time.LocalDateTime
import java.time.ZoneId
import java.time.ZoneOffset
import java.util.concurrent.Executors

private fun meta(json: String): MetaDetail = Titles.parseMeta("""{"meta": $json}""")

/** The Rust build's calendar tests (release dates, the month page), ported. */
class ReleasesTest {
    private val utc = ZoneOffset.UTC

    @Test fun aMomentMovesToTheLocalDayAndABareDateStays() {
        val sydney = ZoneOffset.ofHours(11)
        val newYork = ZoneOffset.ofHours(-5)
        // 9 pm on the 11th in New York is 02:00 UTC on the 12th.
        val aired = "2026-11-12T02:00:00.000Z"
        assertEquals(Day(2026, 11, 11) to Clock(21, 0), Releases.local(aired, newYork))
        assertEquals(Day(2026, 11, 12) to Clock(13, 0), Releases.local(aired, sydney))
        assertEquals(Day(2026, 11, 12) to Clock(13, 0), Releases.local("2026-11-11T21:00:00-05:00", sydney))
        // No time known: the date as given, wherever you are.
        assertEquals(Day(2026, 11, 11) to null, Releases.local("2026-11-11", sydney))
        assertEquals(Day(2026, 11, 11) to null, Releases.local("2026-11-11T00:00:00.000Z", newYork))
        assertNull(Releases.local("soon", sydney))
        assertNull(Day.parse("2026-02-30"))
    }

    @Test fun aMonthPageIsSixWeeksFromAMonday() {
        val days = Releases.month(2026, 11)
        assertEquals(42, days.size)
        // 1 November 2026 is a Sunday: the page starts Monday 26 October.
        assertEquals(Day(2026, 10, 26), days[0])
        assertEquals(Day(2026, 11, 1), days[6])
        assertEquals(Day(2026, 12, 6), days[41])
        assertEquals(Day(2026, 11, 16), Releases.week(Day(2026, 11, 19))[0])
    }

    @Test fun episodesAndFilmsGiveReleases() {
        val series = meta("""{"id": "tt1", "type": "series", "name": "A Show", "released": "2020-01-01", "videos": [{"id": "tt1:3:5", "title": "Five", "season": 3, "episode": 5, "released": "2026-11-18T08:30:00.000Z"}, {"id": "tt1:3:6", "season": 3, "episode": 6}]}""")
        val releases = Releases.of(series, utc)
        assertEquals(1, releases.size)
        assertEquals("S3 E5 · Five", releases[0].label)
        assertEquals("tt1:3:5", releases[0].video)
        assertEquals(Clock(8, 30), releases[0].time)
        assertEquals("S03E05", releases[0].number)
        val doubled = meta("""{"id": "tt3", "type": "series", "name": "B", "videos": [{"id": "tt3:0:31", "title": "The Making Of Episode 1", "season": 0, "episode": 31, "released": "2026-11-11T11:00:00.000Z"}, {"id": "tt3:3:1", "title": "One", "season": 3, "episode": 1, "released": "2026-11-11T11:00:00.000Z"}, {"id": "tt3:3:1", "title": "One", "season": 3, "episode": 1, "released": "2026-11-11T11:00:00.000Z"}]}""")
        val once = Releases.of(doubled, utc)
        assertEquals(1, once.size)
        assertEquals("S3 E1 · One", once[0].label)
        val film = Releases.of(meta("""{"id": "tt2", "type": "movie", "name": "A Film", "released": "2026-12-25T00:00:00.000Z"}"""), utc)
        assertEquals(listOf(Release(Day(2026, 12, 25), null, "", "", "", "")), film)
    }

    @Test fun upcomingShowsEachShowOnceFromToday() {
        val today = Day(2026, 11, 25)
        fun release(day: Int, number: String, name: String = "", month: Int = 11) = Release(Day(2026, month, day), null, "", "v$number", number, name)
        val show = Releases.Title("series", "tt1")
        val other = Releases.Title("series", "tt2")
        val film = Releases.Title("movie", "tt3")
        val cards = Releases.upcoming(
            mapOf(
                show to listOf(release(24, "S03E06"), release(26, "S03E07", "Seven"), release(26, "S03E08"), release(30, "S03E09")),
                other to listOf(release(25, "S01E01", "Pilot")),
                film to listOf(release(10, "", month = 12)),
            ),
            mapOf(show to "A Show", other to "Other", film to "A Film"), mapOf(show to "https://p/1.jpg"), today,
        )
        assertEquals(listOf("Other", "A Show", "A Film"), cards.map { it.title })
        assertEquals(listOf("Today", "Tomorrow", "Dec 10"), cards.map { it.`when` })
        assertTrue(cards[1].episode == "New episodes" && cards[1].badge == "S03E07–E08")
        assertTrue(cards[0].episode == "Pilot" && cards[0].soon)
        assertTrue(cards[2].episode == "Film" && !cards[2].soon)
        assertEquals("https://p/1.jpg", cards[1].picture)
    }

    @Test fun watchNowListsWhatCameOutUntilWatched() {
        fun day(d: Int) = Day(2026, 11, d)
        fun episode(d: Int, n: String) = Release(day(d), null, "", "v$n", n, "Episode $n")
        val show = Releases.Title("series", "show")
        val movie = Releases.Title("movie", "film")
        val season = Releases.Title("series", "season")
        val numbers = (1..10).map { "S01E%02d".format(it) }
        val releases = mapOf(
            show to listOf(episode(2, "S01E01"), episode(12, "S01E02"), episode(19, "S01E03"), episode(26, "S01E04")),
            movie to listOf(Release(day(20), null, "", "", "", "")),
            season to numbers.map { episode(1, it) },
        )
        val added = mapOf(show to day(10), movie to day(1), season to day(20))
        // S01E02 of the show and 8 of the season's 10 watched.
        val watched = { title: Releases.Title, release: Release ->
            (title == show && release.number == "S01E02") || (title == season && release.number <= "S01E08")
        }
        val out = Releases.outNow(releases, added, watched, day(26))
        assertEquals(listOf(movie, show, season), out.map { it.title })
        assertTrue(out[1].first.number == "S01E03" && out[1].count == 1)
        assertTrue(out[2].first.number == "S01E09" && out[2].last.number == "S01E10")
        // Not started and saved after it came out: not on the row.
        assertTrue(Releases.outNow(mapOf(season to releases[season]!!), added, { _, _ -> false }, day(26)).isEmpty())
        assertTrue(Releases.outNow(releases, added, { _, _ -> true }, day(26)).isEmpty())
        val cards = Releases.watchNow(releases, mapOf(show to "Show", movie to "Film", season to "Season"), emptyMap(), added, watched, day(26))
        assertTrue(cards[2].badge == "S01E09–E10" && cards[2].episode == "2 new episodes" && cards[2].whenAt(java.time.Instant.now()) == "Watch now")
        assertEquals("S01E09–S02E01", Releases.runBadge("S01E09", "S02E01"))
    }

    @Test fun releaseDayCountsDownByTheHour() {
        val zone = ZoneId.of("Australia/Adelaide")
        fun at(day: Int, hour: Int, minute: Int) = LocalDateTime.of(2026, 11, day, hour, minute).atZone(zone).toInstant()
        val day = Day(2026, 11, 25)
        val nine = Clock(21, 0)
        assertEquals("In 4 h", Releases.whenLabel(day, nine, at(25, 16, 10), zone))
        assertEquals("In 45 min", Releases.whenLabel(day, nine, at(25, 20, 15), zone))
        assertEquals("Out now", Releases.whenLabel(day, nine, at(25, 21, 0), zone))
        assertEquals("Today", Releases.whenLabel(day, null, at(25, 9, 0), zone))
        assertEquals("Tomorrow", Releases.whenLabel(day, nine, at(24, 23, 0), zone))
        assertEquals("In 6 days", Releases.whenLabel(day, null, at(19, 9, 0), zone))
        assertEquals("Nov 25", Releases.whenLabel(day, null, at(18, 9, 0), zone))
    }
}

/** The Rust build's Home tests (home.rs), ported. */
class HomeTopTest {
    @Test fun episodesLeftCountFromTheCurrentOne() {
        fun video(s: Int, e: Int, day: String) = Video("tt1:$s:$e", season = s, episode = e, released = day)
        val videos = listOf(video(0, 1, "2020-01-01"), video(1, 1, "2020-01-01"), video(1, 2, "2020-01-08"), video(1, 3, "2020-01-15"), video(2, 1, "2030-01-01"))
        assertEquals(2, HomeTop.episodesLeft(videos, "tt1:1:1", setOf("tt1:1:3"), "2026-11-19"))
        assertEquals(0, HomeTop.episodesLeft(videos, "tt9:1:1", setOf("tt1:1:3"), "2026-11-19"))
        assertEquals("S03E06", HomeTop.episodeBadge("tt1:3:6"))
        assertEquals("", HomeTop.episodeBadge("tt1"))
    }

    @Test fun theBannerSaysWhatIsKnown() {
        val film = MetaDetail("tt1", "movie", "A Film", releaseInfo = "2026", genres = listOf("Adventure", "Action", "Fantasy", "Drama"), rating = "8.2")
        val scores = Omdb.Scores("8.0", "94%", "78", "R", "173 min")
        assertEquals("2026  ·  Adventure, Action, Fantasy  ·  173 min  ·  R", HomeTop.bannerFacts(film, scores))
        assertEquals("2026  ·  Adventure, Action, Fantasy", HomeTop.bannerFacts(film, null))
        assertEquals(listOf("IMDb 8.2", "TMDB 7.8", "RT 94%", "MC 78"), HomeTop.scorePills(film, 7.84, scores).map { "${it.label} ${it.value}" })
        assertEquals(listOf("IMDb"), HomeTop.scorePills(film, 0.0, null).map { it.label })
    }

    @Test fun continueCardsNameTheEpisodeAndWhatIsLeft() {
        val progress = WatchProgress("series", "tt1", "tt1:1:2", "A Show", "S1 E2 · Second", "https://p/poster.jpg", 600.0, 3000.0, false, 1)
        val film = WatchProgress("movie", "tt5", "tt5", "A Film", "", null, 10.0, 6000.0, false, 2)
        val key = Releases.Title("series", "tt1")
        val videos = (1..4).map { Video("tt1:1:$it", season = 1, episode = it, released = "2020-01-0$it") }
        val cards = HomeTop.cards(listOf(progress, film), mapOf(key to HomeTop.Known("https://p/wide.jpg", videos)), { setOf("tt1:1:3") }, "2026-01-01")
        assertTrue(cards[0].episode == "Second" && cards[0].badge == "S01E02" && cards[0].left == 2)
        assertEquals("https://p/wide.jpg", cards[0].picture)
        assertEquals(0.2, cards[0].progress, 0.0001)
        assertTrue("a bar is always seen", cards[1].episode == "" && cards[1].badge == "" && cards[1].progress == 0.02)
    }
}

class LibraryTest {
    private val thread = Executors.newSingleThreadExecutor().asCoroutineDispatcher()

    private fun store(): Store = Store.open(File.createTempFile("haven-test-", ".sqlite3").also { it.delete(); it.deleteOnExit() })

    private fun library(web: FakeWeb, kept: Kept = Kept(Files.createTempDirectory("haven-kept").toFile()), store: Store = store()): Pair<Library, CoroutineScope> {
        val scope = CoroutineScope(SupervisorJob() + thread)
        return Library(store, Secrets(), web.http, kept, scope) to scope
    }

    /** Runs `block` on the library's own thread (as the main thread in the app). */
    private fun <T> on(block: () -> T): T = kotlinx.coroutines.runBlocking(thread) { block() }

    private fun waitFor(test: () -> Boolean) {
        repeat(100) { if (on(test)) return; Thread.sleep(50) }
    }

    private val popular = """{"metas": [{"id": "tt0000101", "type": "movie", "name": "One", "imdbRating": "7.1"},
        {"id": "tt0000102", "type": "movie", "name": "Two"}, {"id": "tt0000103", "type": "movie", "name": "Three"},
        {"id": "tt0000102", "type": "movie", "name": "Two"}]}"""

    @Test fun homeFillsFromCinemetaAndKeepsItForTheNextStart() {
        val web = FakeWeb()
        for (catalog in Cinemeta.fallback) web.answer(ResourceUrl.catalog(Cinemeta.URL, catalog), popular)
        val (library, scope) = library(web)
        on { library.reloadHome() }
        assertEquals(listOf("Popular films", "Popular series", "Featured films", "Featured series"), library.homeBoard.shelves.map { it.title })
        assertTrue(library.homeBoard.shelves.all { it.loading })
        waitFor { library.homeBoard.settled == 4 }
        val first = library.homeBoard.shelves[0]
        assertFalse(first.loading)
        assertEquals("repeats are dropped", listOf("One", "Two", "Three"), first.items.map { it.name })
        // The banner takes the first shelf with 3 titles.
        assertEquals(listOf("One", "Two", "Three"), library.hero.map { it.title })
        // The next start shows the kept titles at once.
        val (again, scope2) = library(FakeWeb(), library.kept)
        on {
            again.homeShowing(false)
            again.reloadHome()
        }
        assertEquals(listOf("One", "Two", "Three"), again.homeBoard.shelves[0].items.map { it.name })
        assertFalse(again.homeBoard.shelves[0].loading)
        scope.cancel()
        scope2.cancel()
    }

    @Test fun aShelfThatFailsSaysSo() {
        val (library, scope) = library(FakeWeb())
        on { library.fill(library.homeBoard, listOf(ShelfPlan("Broken films", "Nowhere", ShelfFrom.Catalogue("https://nowhere.test/manifest.json", Catalog("movie", "x"))))) }
        waitFor { library.homeBoard.settled == 1 }
        assertEquals("Could not load this: the server answered with error 404.", library.homeBoard.shelves[0].error)
        // No TMDB key: no TMDB shelves.
        on { library.reloadCatalog() }
        assertTrue(library.catalogBoard.shelves.isEmpty())
        scope.cancel()
    }

    @Test fun continueWatchingAndTheWatchlistComeFromTheDatabase() {
        val store = store()
        val (library, scope) = library(FakeWeb(), store = store)
        store.saveProgress(WatchProgress("movie", "tt7", "tt7", "Seven", "", "https://p/7.jpg", 900.0, 3600.0, false, 5))
        store.addToWatchlist(Saved("movie", "tt8", "Eight", "2026", "https://p/8.jpg", 1))
        on {
            library.reloadContinue()
            library.reloadWatchlist()
        }
        assertEquals(listOf("Seven"), library.continueCards.map { it.title })
        assertEquals("https://p/7.jpg", library.continueCards[0].picture)
        assertEquals(listOf("Eight"), library.watchlist.map { it.title })
        on { library.removeContinue("movie", "tt7") }
        assertTrue(library.continueCards.isEmpty())
        assertFalse(library.filmWatched("movie", "tt7"))
        store.saveProgress(WatchProgress("movie", "tt9", "tt9", "Nine", "", null, 0.0, 0.0, true, 6))
        assertTrue(library.filmWatched("movie", "tt9"))
        assertFalse(library.filmWatched("series", "tt9"))
        scope.cancel()
    }

    @Test fun theLoadingScreenWaitsForHomesShelves() {
        val (library, scope) = library(FakeWeb())
        assertEquals("Cinemeta is a source: its shelves are to come", 0.0, library.startupProgress(true), 0.0)
        on { library.reloadHome() }
        assertTrue(library.startupProgress(true) < 1)
        assertEquals("another page shows: Home is not waited for", 1.0, library.startupProgress(false), 0.0)
        scope.cancel()
    }

    @Test fun searchSaysWhenNothingCanBeSearched() {
        assertEquals("", Library.searchNote(1, 2, true))
        assertEquals("None of your add-ons can be searched.", Library.searchNote(1, 0, true))
        assertEquals("Nothing you have added can be searched.", Library.searchNote(1, 0, false))
        assertFalse(Library.emptyText(false).contains("add-on"))
    }
}
