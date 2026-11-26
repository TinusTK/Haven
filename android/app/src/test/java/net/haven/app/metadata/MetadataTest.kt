package net.haven.app.metadata

import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import net.haven.app.core.Store
import net.haven.app.testing.FakeWeb
import net.haven.app.titles.MetaDetail
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

private fun json(text: String): JsonObject = Json.parseToJsonElement(text) as JsonObject
private val empty = buildJsonObject { }

/** The Rust build's metadata tests (haven-metadata lib, catalog, omdb),
 *  ported as the Swift build has them. */
class TmdbTest {
    private val images = "https://img.example/t/p"

    @Test
    fun onlyImdbIdsAreLookedUp() {
        assertTrue(Tmdb.isImdbId("tt0903747"))
        for (id in listOf("", "tt", "tt12a", "srv:1:abc", "hv1", "tt1234567890123")) assertFalse(id, Tmdb.isImdbId(id))
    }

    @Test
    fun findsTheFilmOrTheSeriesRecord() {
        val found = json("""{"movie_results": [{"id": 11}], "tv_results": [{"id": 22}]}""")
        assertEquals(11L, Tmdb.foundId(found, series = false))
        assertEquals(22L, Tmdb.foundId(found, series = true))
        assertNull(Tmdb.foundId(json("""{"movie_results": []}"""), series = false))
    }

    @Test
    fun readsAFilm() {
        val details = Tmdb.readDetails(
            json(
                """
            {"overview": " About it. ", "runtime": 118, "vote_average": 7.8,
             "genres": [{"name": "Drama"}, {"name": "Crime"}],
             "backdrop_path": "/b.jpg", "poster_path": "/p.jpg",
             "credits": {"cast": [{"name": "An Actor"}, {"name": "Another"}],
                         "crew": [{"name": "A Writer", "job": "Writer"}, {"name": "A Director", "job": "Director"}]}}
            """,
            ),
        )
        assertEquals("About it.", details.overview)
        assertEquals(118, details.runtimeMinutes)
        assertEquals(7.8, details.rating!!, 0.0)
        assertEquals(listOf("Drama", "Crime"), details.genres)
        assertEquals(listOf("An Actor", "Another"), details.cast)
        assertEquals(listOf("A Director"), details.directors)
        assertEquals("https://image.tmdb.org/t/p/w1280/b.jpg", details.backdrop)
        assertEquals("https://image.tmdb.org/t/p/w500/p.jpg", details.poster)
    }

    @Test
    fun readsASeriesAndCopesWithGaps() {
        val details = Tmdb.readDetails(json("""{"episode_run_time": [47], "vote_average": 0.0, "created_by": [{"name": "A Creator"}], "overview": ""}"""))
        assertEquals(47, details.runtimeMinutes)
        assertNull(details.rating)
        assertNull(details.overview)
        assertEquals(listOf("A Creator"), details.directors)
        assertEquals(TmdbDetails(), Tmdb.readDetails(empty))
    }

    @Test
    fun idsRoundTrip() {
        assertEquals("tmdb:movie:603", Tmdb.titleId(false, 603))
        assertEquals(false to 603L, Tmdb.splitId("tmdb:movie:603"))
        assertEquals(true to 1396L, Tmdb.splitId(Tmdb.titleId(true, 1396)))
        for (id in listOf("tt0903747", "tmdb:person:5", "tmdb:tv:x", "tmdb:tv", "srv:1:a")) assertNull(id, Tmdb.splitId(id))
    }

    @Test
    fun aPageBecomesPosters() {
        val found = json(
            """
            {"results": [
              {"id": 11, "title": "A Film", "poster_path": "/a.jpg", "release_date": "2021-03-04", "vote_average": 7.25},
              {"id": 12, "name": "A Show", "first_air_date": "2019-01-01"},
              {"id": 13},
              {"title": "No id"}]}
            """,
        )
        val films = Tmdb.readPage(found, series = false, images = images)
        assertEquals(2, films.size)
        assertTrue(films[0].id == "tmdb:movie:11" && films[0].kind == "movie")
        assertEquals("https://img.example/t/p/w342/a.jpg", films[0].poster)
        assertEquals("2021", films[0].releaseInfo)
        assertEquals("7.2", films[0].rating)
        assertTrue(films[1].name == "A Show" && films[1].poster == null && films[1].rating == null)
        assertEquals("tmdb:tv:12", Tmdb.readPage(found, series = true, images = images)[1].id)
        assertTrue(Tmdb.readPage(empty, series = false, images = images).isEmpty())
    }

    @Test
    fun genresAreRead() {
        val found = json("""{"genres": [{"id": 18, "name": "Drama"}, {"id": 35}, {"name": "No id"}]}""")
        assertEquals(listOf(Genre(18, "Drama")), Tmdb.readGenres(found))
    }

    @Test
    fun aFilmRecordBecomesAPageOfDetails() {
        val record = json(
            """
            {"title": "A Film", "overview": "About it.", "runtime": 118, "vote_average": 7.84,
             "release_date": "2021-03-04", "imdb_id": "tt0000011",
             "genres": [{"name": "Drama"}], "poster_path": "/p.jpg", "backdrop_path": "/b.jpg",
             "credits": {"cast": [{"name": "An Actor"}], "crew": [{"name": "A Director", "job": "Director"}]}}
            """,
        )
        val meta = Tmdb.readTitle(record, series = false, number = 11, images = images)!!
        assertTrue(meta.id == "tmdb:movie:11" && meta.kind == "movie" && meta.name == "A Film")
        assertEquals("tt0000011", meta.imdbId)
        assertEquals("11", meta.tmdbId)
        assertEquals("118 min", meta.runtime)
        assertEquals("7.8", meta.rating)
        assertEquals("2021", meta.releaseInfo)
        assertEquals("2021-03-04", meta.released)
        assertEquals("https://img.example/t/p/w500/p.jpg", meta.poster)
        assertEquals("https://img.example/t/p/w1280/b.jpg", meta.background)
        assertTrue(meta.cast == listOf("An Actor") && meta.director == listOf("A Director"))
        assertNull(Tmdb.readTitle(empty, series = false, number = 1, images = images))
    }

    @Test
    fun aSeriesListsItsSeasonsAndEpisodes() {
        val record = json(
            """
            {"name": "A Show", "first_air_date": "2019-01-01", "external_ids": {"imdb_id": "tt0000022"},
             "seasons": [{"season_number": 0, "episode_count": 2}, {"season_number": 1, "episode_count": 8},
                         {"season_number": 2, "episode_count": 0}]}
            """,
        )
        assertEquals(listOf(0, 1), Tmdb.seasonNumbers(record))
        val meta = Tmdb.readTitle(record, series = true, number = 22, images = images)!!
        assertEquals("tt0000022", meta.imdbId)
        val answer = json(
            """
            {"season/1": {"episodes": [{"episode_number": 1, "name": "Pilot", "air_date": "2019-01-01"},
                                       {"episode_number": 2}, {"name": "No number"}]}}
            """,
        )
        val episodes = Tmdb.readSeason(answer, meta, 22, 1)
        assertEquals(2, episodes.size)
        assertEquals("tt0000022:1:1", episodes[0].id)
        assertTrue(episodes[0].season == 1 && episodes[0].episode == 1)
        assertTrue(episodes[0].title == "Pilot" && episodes[0].released == "2019-01-01")
        assertTrue(Tmdb.readSeason(answer, meta, 22, 3).isEmpty())
        // Without an IMDb id the episode ids stay Haven's own.
        val bare = Tmdb.readTitle(json("""{"name": "Bare"}"""), series = true, number = 23, images = images)!!
        assertEquals("tmdb:tv:23:1:1", Tmdb.readSeason(answer, bare, 23, 1)[0].id)
    }

    @Test
    fun offersComeFromTheFirstCountryThatHasAny() {
        val answer = json(
            """
            {"results": {
              "AU": {"link": "https://www.themoviedb.org/movie/11/watch?locale=AU"},
              "US": {"link": "https://www.themoviedb.org/movie/11/watch?locale=US",
                     "flatrate": [{"provider_name": "Service One"}],
                     "rent": [{"provider_name": "Shop Two"}, {"provider_name": "Service One"}],
                     "buy": [{"provider_name": "Shop Two"}]}}}
            """,
        )
        val offers = Tmdb.readOffers(answer, listOf("au", "US"))!!
        assertEquals("US", offers.country)
        assertEquals("https://www.themoviedb.org/movie/11/watch?locale=US", offers.link)
        assertEquals(
            listOf(Service("Service One", listOf("Subscription", "Rent")), Service("Shop Two", listOf("Rent", "Buy"))),
            offers.services,
        )
        assertNull(Tmdb.readOffers(answer, listOf("GB")))
        assertNull(Tmdb.readOffers(empty, listOf("au", "US")))
    }

    @Test
    fun searchWordsAreEscaped() {
        assertEquals("the%20matrix%20%26%20more", Tmdb.escaped(" the matrix & more "))
    }

    @Test
    fun keysTravelAsTmdbWantsThem() = runBlocking {
        val web = FakeWeb()
        val page = """{"results": [{"id": 5, "title": "Five"}]}"""
        // The short key goes in the address…
        web.answer("https://tmdb.test/3/movie/popular?page=1&api_key=abc123", page)
        val short = Tmdb(" abc123\n", web.http, api = "https://tmdb.test/3")
        assertEquals(listOf("Five"), short.list(TmdbList.Popular, series = false, page = 0).map { it.name })
        // …the long token in a header, never in the address.
        web.answer("https://tmdb.test/3/tv/top_rated?page=2", page)
        val token = Tmdb("eyJ.abc-def_1", web.http, api = "https://tmdb.test/3")
        assertEquals(listOf("tmdb:tv:5"), token.list(TmdbList.TopRated, series = true, page = 2).map { it.id })
        assertEquals("Bearer eyJ.abc-def_1", web.headers("https://tmdb.test/3/tv/top_rated?page=2")?.get("Authorization"))
        // A refused key.
        web.answer("https://tmdb.test/3/genre/movie/list?api_key=abc123", "{}", status = 401)
        assertEquals(MetadataError.BadKey(), runCatching { short.genres(series = false) }.exceptionOrNull())
    }

    @Test
    fun aSeriesIsFetchedWithItsSeasons() = runBlocking {
        val web = FakeWeb()
        val tmdb = Tmdb("k1", web.http, api = "https://tmdb2.test/3")
        web.answer(
            "https://tmdb2.test/3/tv/22?append_to_response=credits,external_ids&api_key=k1",
            """{"name": "A Show", "external_ids": {"imdb_id": "tt0000022"},
             "seasons": [{"season_number": 0, "episode_count": 1}, {"season_number": 1, "episode_count": 2}]}""",
        )
        web.answer(
            "https://tmdb2.test/3/tv/22?append_to_response=season/0,season/1&api_key=k1",
            """{"season/0": {"episodes": [{"episode_number": 1}]},
             "season/1": {"episodes": [{"episode_number": 1}, {"episode_number": 2}]}}""",
        )
        val meta = tmdb.title(series = true, number = 22)
        assertEquals(listOf("tt0000022:0:1", "tt0000022:1:1", "tt0000022:1:2"), meta.videos.map { it.id })
        assertEquals(listOf(1, 0), meta.seasons)
    }
}

class OmdbTest {
    @Test
    fun scoresAreReadAndUnknownsLeftOut() {
        val scores = Omdb.readScores(
            json(
                """{"Response": "True", "imdbRating": "8.4", "Metascore": "N/A", "Rated": "R", "Runtime": "173 min",
             "Ratings": [{"Source": "Internet Movie Database", "Value": "8.4/10"}, {"Source": "Rotten Tomatoes", "Value": "94%"}]}""",
            ),
        )!!
        assertEquals("8.4", scores.imdb)
        assertEquals("94%", scores.rottenTomatoes)
        assertNull(scores.metacritic)
        assertEquals("R", scores.rated)
        assertEquals("173 min", scores.runtime)
        assertNull(Omdb.readScores(json("""{"Response": "False"}""")))
        assertNull(Omdb.readScores(json("""{"Rated": "Not Rated"}"""))?.rated)
    }

    @Test
    fun scoresAreKeptWithTheRustBuildsNames() {
        val text = Store.json.encodeToString(Omdb.Scores.serializer(), Omdb.Scores(rottenTomatoes = "94%"))
        assertTrue(text, text.contains("\"rotten_tomatoes\":\"94%\""))
    }

    @Test
    fun scoresAreAskedForByImdbId() = runBlocking {
        val web = FakeWeb().apply { answer("https://www.omdbapi.com/?i=tt0000099&apikey=abcd1234", """{"Response": "True", "Metascore": "78"}""") }
        assertEquals("78", Omdb.scores("abcd1234", "tt0000099", web.http)?.metacritic)
        assertNull(Omdb.scores("abcd1234", "tmdb:movie:1", web.http))
    }
}

class WhereToWatchTest {
    @Test
    fun offersFallIntoTheirGroupsWithOurTiles() {
        val rows = WhereToWatch.tiles(
            listOf(
                Service("Netflix", listOf("Subscription")),
                Service("Amazon Prime Video", listOf("Subscription")),
                Service("Amazon Prime Video with Ads", listOf("Subscription")),
                Service("Apple TV", listOf("Rent", "Buy")),
                Service("Tubi TV", listOf("Free with ads")),
                Service("Some Shop", listOf("Buy")),
            ),
        )
        assertEquals(
            listOf("Stream|Netflix", "|Prime Video", "Free|Tubi TV", "Rent|Apple TV", "Buy|Apple TV", "|Some Shop"),
            rows.map { "${it.group}|${it.name}" },
        )
        assertTrue(rows[0].letters == "N" && rows[0].tint == 0x1a0709)
        assertTrue(rows[5].letters == "S" && rows[5].ink == 0xf0c36a)
        assertEquals("Rent, Buy", rows[3].ways)
    }

    @Test
    fun justWatchLinksUseItsCountryNames() {
        assertEquals("https://www.justwatch.com/uk/search?q=Dune%3A%20Part%20Two", WhereToWatch.justWatchLink("GB", "Dune: Part Two"))
        assertEquals("https://www.justwatch.com/au/search?q=Up", WhereToWatch.justWatchLink("AU", "Up"))
    }

    @Test
    fun theRegionComesFromAnyWayOfWritingALocale() {
        assertEquals("AU", WhereToWatch.region("en-AU"))
        assertEquals("GB", WhereToWatch.region("en_GB.UTF-8"))
        assertEquals("HK", WhereToWatch.region("zh-Hant-HK"))
        assertNull(WhereToWatch.region("en"))
        assertNull(WhereToWatch.region("C.UTF-8"))
    }

    @Test
    fun theCountryIsPickedOrAutomatic() {
        val store = Store.inMemory()
        assertEquals(0, WhereToWatch.settingsIndex(store))
        assertTrue(WhereToWatch.settingsList().first().startsWith("Automatic ("))
        assertEquals(41, WhereToWatch.settingsList().size)
        WhereToWatch.choose(2, store) // Australia
        assertEquals("AU", WhereToWatch.country(store))
        assertEquals("AU", store.get<String>(WhereToWatch.COUNTRY_KEY))
        assertEquals(2, WhereToWatch.settingsIndex(store))
        WhereToWatch.choose(0, store)
        assertNull(WhereToWatch.chosenCountry(store))
        store.put(WhereToWatch.COUNTRY_KEY, "XX") // not offered: ignored
        assertNull(WhereToWatch.chosenCountry(store))
    }

    @Test
    fun lookingUpFallsBackToOtherCountries() = runBlocking {
        val web = FakeWeb().apply {
            answer("https://tmdb3.test/3/find/tt0000031?external_source=imdb_id&api_key=k2", """{"movie_results": [{"id": 31}]}""")
            answer("https://tmdb3.test/3/movie/31/watch/providers?api_key=k2", """{"results": {"GB": {"buy": [{"provider_name": "Shop"}]}}}""")
        }
        val tmdb = Tmdb("k2", web.http, api = "https://tmdb3.test/3")
        val found = WhereToWatch.lookUp(MetaDetail("tt0000031", "movie", "Thirty One"), tmdb, "AU")
        assertEquals("United Kingdom (nothing in Australia)", found.note)
        assertEquals("https://www.justwatch.com/uk/search?q=Thirty%20One", found.link)
        assertEquals(listOf("Buy"), found.tiles.map { it.group })
        // Unknown to the stand-in: TMDB answers 404.
        val failed = WhereToWatch.lookUp(MetaDetail("tt0000032", "movie", "Thirty One"), tmdb, "AU")
        assertEquals("Could not look this up: TMDB answered with error 404.", failed.note)
        assertTrue(failed.tiles.isEmpty())
    }
}
