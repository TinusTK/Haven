import Foundation
import Testing
@testable import Haven

/// JSON text as TMDB-shaped test answers.
private func json(_ text: String) -> [String: Any] {
    try! JSONSerialization.jsonObject(with: Data(text.utf8)) as! [String: Any]
}

/// The Rust build's metadata tests (haven-metadata lib, catalog, omdb),
/// ported, with answers made up in the shape TMDB answers with.
struct TMDBTests {
    private let images = "https://img.example/t/p"

    @Test func onlyIMDbIDsAreLookedUp() {
        #expect(TMDB.isIMDbID("tt0903747"))
        for id in ["", "tt", "tt12a", "srv:1:abc", "hv1", "tt1234567890123"] {
            #expect(!TMDB.isIMDbID(id), "\(id)")
        }
    }

    @Test func findsTheFilmOrTheSeriesRecord() {
        let found = json(#"{"movie_results": [{"id": 11}], "tv_results": [{"id": 22}]}"#)
        #expect(TMDB.foundID(found, series: false) == 11)
        #expect(TMDB.foundID(found, series: true) == 22)
        #expect(TMDB.foundID(json(#"{"movie_results": []}"#), series: false) == nil)
    }

    @Test func readsAFilm() {
        let record = json("""
            {"overview": " About it. ", "runtime": 118, "vote_average": 7.8,
             "genres": [{"name": "Drama"}, {"name": "Crime"}],
             "backdrop_path": "/b.jpg", "poster_path": "/p.jpg",
             "credits": {"cast": [{"name": "An Actor"}, {"name": "Another"}],
                         "crew": [{"name": "A Writer", "job": "Writer"}, {"name": "A Director", "job": "Director"}]}}
            """)
        let details = TMDB.readDetails(record)
        #expect(details.overview == "About it.")
        #expect(details.runtimeMinutes == 118)
        #expect(details.rating == 7.8)
        #expect(details.genres == ["Drama", "Crime"])
        #expect(details.cast == ["An Actor", "Another"])
        #expect(details.directors == ["A Director"])
        #expect(details.backdrop == "https://image.tmdb.org/t/p/w1280/b.jpg")
        #expect(details.poster == "https://image.tmdb.org/t/p/w500/p.jpg")
    }

    @Test func readsASeriesAndCopesWithGaps() {
        let details = TMDB.readDetails(json(#"{"episode_run_time": [47], "vote_average": 0.0, "created_by": [{"name": "A Creator"}], "overview": ""}"#))
        #expect(details.runtimeMinutes == 47)
        #expect(details.rating == nil)
        #expect(details.overview == nil)
        #expect(details.directors == ["A Creator"])
        #expect(TMDB.readDetails([:]) == TMDBDetails())
    }

    @Test func idsRoundTrip() {
        #expect(TMDB.titleID(series: false, number: 603) == "tmdb:movie:603")
        #expect(TMDB.splitID("tmdb:movie:603")! == (false, 603))
        #expect(TMDB.splitID(TMDB.titleID(series: true, number: 1396))! == (true, 1396))
        for id in ["tt0903747", "tmdb:person:5", "tmdb:tv:x", "tmdb:tv", "srv:1:a"] {
            #expect(TMDB.splitID(id) == nil, "\(id)")
        }
    }

    @Test func aPageBecomesPosters() {
        let found = json("""
            {"results": [
              {"id": 11, "title": "A Film", "poster_path": "/a.jpg", "release_date": "2021-03-04", "vote_average": 7.25},
              {"id": 12, "name": "A Show", "first_air_date": "2019-01-01"},
              {"id": 13},
              {"title": "No id"}]}
            """)
        let films = TMDB.readPage(found, series: false, images: images)
        #expect(films.count == 2)
        #expect(films[0].id == "tmdb:movie:11" && films[0].kind == "movie")
        #expect(films[0].poster == "https://img.example/t/p/w342/a.jpg")
        #expect(films[0].releaseInfo == "2021")
        #expect(films[0].rating == "7.2")
        #expect(films[1].name == "A Show" && films[1].poster == nil && films[1].rating == nil)
        #expect(TMDB.readPage(found, series: true, images: images)[1].id == "tmdb:tv:12")
        #expect(TMDB.readPage([:], series: false, images: images).isEmpty)
    }

    @Test func genresAreRead() {
        let found = json(#"{"genres": [{"id": 18, "name": "Drama"}, {"id": 35}, {"name": "No id"}]}"#)
        #expect(TMDB.readGenres(found) == [Genre(id: 18, name: "Drama")])
    }

    @Test func aFilmRecordBecomesAPageOfDetails() throws {
        let record = json("""
            {"title": "A Film", "overview": "About it.", "runtime": 118, "vote_average": 7.84,
             "release_date": "2021-03-04", "imdb_id": "tt0000011",
             "genres": [{"name": "Drama"}], "poster_path": "/p.jpg", "backdrop_path": "/b.jpg",
             "credits": {"cast": [{"name": "An Actor"}], "crew": [{"name": "A Director", "job": "Director"}]}}
            """)
        let meta = try #require(TMDB.readTitle(record, series: false, number: 11, images: images))
        #expect(meta.id == "tmdb:movie:11" && meta.kind == "movie" && meta.name == "A Film")
        #expect(meta.imdbID == "tt0000011")
        #expect(meta.tmdbID == "11")
        #expect(meta.runtime == "118 min")
        #expect(meta.rating == "7.8")
        #expect(meta.releaseInfo == "2021")
        #expect(meta.released == "2021-03-04")
        #expect(meta.poster == "https://img.example/t/p/w500/p.jpg")
        #expect(meta.background == "https://img.example/t/p/w1280/b.jpg")
        #expect(meta.cast == ["An Actor"] && meta.director == ["A Director"])
        #expect(TMDB.readTitle([:], series: false, number: 1, images: images) == nil)
    }

    @Test func aSeriesListsItsSeasonsAndEpisodes() throws {
        let record = json("""
            {"name": "A Show", "first_air_date": "2019-01-01", "external_ids": {"imdb_id": "tt0000022"},
             "seasons": [{"season_number": 0, "episode_count": 2}, {"season_number": 1, "episode_count": 8},
                         {"season_number": 2, "episode_count": 0}]}
            """)
        #expect(TMDB.seasonNumbers(record) == [0, 1])
        let meta = try #require(TMDB.readTitle(record, series: true, number: 22, images: images))
        #expect(meta.imdbID == "tt0000022")

        let answer = json("""
            {"season/1": {"episodes": [{"episode_number": 1, "name": "Pilot", "air_date": "2019-01-01"},
                                       {"episode_number": 2}, {"name": "No number"}]}}
            """)
        let episodes = TMDB.readSeason(answer, meta: meta, number: 22, season: 1)
        #expect(episodes.count == 2)
        #expect(episodes[0].id == "tt0000022:1:1")
        #expect(episodes[0].season == 1 && episodes[0].episode == 1)
        #expect(episodes[0].title == "Pilot" && episodes[0].released == "2019-01-01")
        #expect(TMDB.readSeason(answer, meta: meta, number: 22, season: 3).isEmpty)

        // Without an IMDb id the episode ids stay Haven's own.
        let bare = try #require(TMDB.readTitle(json(#"{"name": "Bare"}"#), series: true, number: 23, images: images))
        #expect(TMDB.readSeason(answer, meta: bare, number: 23, season: 1)[0].id == "tmdb:tv:23:1:1")
    }

    @Test func offersComeFromTheFirstCountryThatHasAny() throws {
        let answer = json("""
            {"results": {
              "AU": {"link": "https://www.themoviedb.org/movie/11/watch?locale=AU"},
              "US": {"link": "https://www.themoviedb.org/movie/11/watch?locale=US",
                     "flatrate": [{"provider_name": "Service One"}],
                     "rent": [{"provider_name": "Shop Two"}, {"provider_name": "Service One"}],
                     "buy": [{"provider_name": "Shop Two"}]}}}
            """)
        let offers = try #require(TMDB.readOffers(answer, countries: ["au", "US"]))
        #expect(offers.country == "US")
        #expect(offers.link == "https://www.themoviedb.org/movie/11/watch?locale=US")
        #expect(offers.services == [Service(name: "Service One", ways: ["Subscription", "Rent"]),
                                    Service(name: "Shop Two", ways: ["Rent", "Buy"])])
        #expect(TMDB.readOffers(answer, countries: ["GB"]) == nil)
        #expect(TMDB.readOffers([:], countries: ["au", "US"]) == nil)
    }

    @Test func searchWordsAreEscaped() {
        #expect(TMDB.escaped(" the matrix & more ") == "the%20matrix%20%26%20more")
    }

    @Test func keysTravelAsTMDBWantsThem() async throws {
        let http = HTTP(protocolClasses: [FixtureServer.self])
        let page = #"{"results": [{"id": 5, "title": "Five"}]}"#
        // The short key goes in the address…
        FixtureServer.answer("https://tmdb.test/3/movie/popular?page=1&api_key=abc123", page)
        var short = TMDB(key: " abc123\n", http: http)
        short.api = "https://tmdb.test/3"
        #expect(try await short.list(.popular, series: false, page: 0).map(\.name) == ["Five"])
        // …the long token in a header, never in the address.
        FixtureServer.answer("https://tmdb.test/3/tv/top_rated?page=2", page)
        var token = TMDB(key: "eyJ.abc-def_1", http: http)
        token.api = "https://tmdb.test/3"
        #expect(try await token.list(.topRated, series: true, page: 2).map(\.id) == ["tmdb:tv:5"])
        #expect(FixtureServer.headers("https://tmdb.test/3/tv/top_rated?page=2")?["Authorization"] == "Bearer eyJ.abc-def_1")
        // A refused key.
        FixtureServer.answer("https://tmdb.test/3/genre/movie/list?api_key=abc123", status: 401, "{}")
        await #expect(throws: MetadataError.badKey) { _ = try await short.genres(series: false) }
    }

    @Test func aSeriesIsFetchedWithItsSeasons() async throws {
        let http = HTTP(protocolClasses: [FixtureServer.self])
        var tmdb = TMDB(key: "k1", http: http)
        tmdb.api = "https://tmdb2.test/3"
        FixtureServer.answer("https://tmdb2.test/3/tv/22?append_to_response=credits,external_ids&api_key=k1", """
            {"name": "A Show", "external_ids": {"imdb_id": "tt0000022"},
             "seasons": [{"season_number": 0, "episode_count": 1}, {"season_number": 1, "episode_count": 2}]}
            """)
        FixtureServer.answer("https://tmdb2.test/3/tv/22?append_to_response=season/0,season/1&api_key=k1", """
            {"season/0": {"episodes": [{"episode_number": 1}]},
             "season/1": {"episodes": [{"episode_number": 1}, {"episode_number": 2}]}}
            """)
        let meta = try await tmdb.title(series: true, number: 22)
        #expect(meta.videos.map(\.id) == ["tt0000022:0:1", "tt0000022:1:1", "tt0000022:1:2"])
        #expect(meta.seasons == [1, 0])
    }
}

struct OMDbTests {
    @Test func scoresAreReadAndUnknownsLeftOut() throws {
        let answer = json("""
            {"Response": "True", "imdbRating": "8.4", "Metascore": "N/A", "Rated": "R", "Runtime": "173 min",
             "Ratings": [{"Source": "Internet Movie Database", "Value": "8.4/10"}, {"Source": "Rotten Tomatoes", "Value": "94%"}]}
            """)
        let scores = try #require(OMDb.readScores(answer))
        #expect(scores.imdb == "8.4")
        #expect(scores.rottenTomatoes == "94%")
        #expect(scores.metacritic == nil)
        #expect(scores.rated == "R")
        #expect(scores.runtime == "173 min")
        #expect(OMDb.readScores(json(#"{"Response": "False"}"#)) == nil)
        #expect(OMDb.readScores(json(#"{"Rated": "Not Rated"}"#))?.rated == nil)
    }

    @Test func scoresAreKeptWithTheRustBuildsNames() throws {
        let text = String(decoding: try Store.encoder.encode(OMDb.Scores(rottenTomatoes: "94%")), as: UTF8.self)
        #expect(text.contains("\"rotten_tomatoes\":\"94%\""))
    }

    @Test func scoresAreAskedForByIMDbID() async throws {
        let http = HTTP(protocolClasses: [FixtureServer.self])
        FixtureServer.answer("https://www.omdbapi.com/?i=tt0000099&apikey=abcd1234", #"{"Response": "True", "Metascore": "78"}"#)
        #expect(try await OMDb.scores(key: "abcd1234", imdb: "tt0000099", http: http)?.metacritic == "78")
        #expect(try await OMDb.scores(key: "abcd1234", imdb: "tmdb:movie:1", http: http) == nil)
    }
}

struct WhereToWatchTests {
    @Test func offersFallIntoTheirGroupsWithOurTiles() {
        let services = [
            Service(name: "Netflix", ways: ["Subscription"]),
            Service(name: "Amazon Prime Video", ways: ["Subscription"]),
            Service(name: "Amazon Prime Video with Ads", ways: ["Subscription"]),
            Service(name: "Apple TV", ways: ["Rent", "Buy"]),
            Service(name: "Tubi TV", ways: ["Free with ads"]),
            Service(name: "Some Shop", ways: ["Buy"]),
        ]
        let rows = WhereToWatch.tiles(for: services)
        let seen = rows.map { "\($0.group)|\($0.name)" }
        #expect(seen == ["Stream|Netflix", "|Prime Video", "Free|Tubi TV", "Rent|Apple TV", "Buy|Apple TV", "|Some Shop"])
        #expect(rows[0].letters == "N" && rows[0].tint == 0x1a0709)
        #expect(rows[5].letters == "S" && rows[5].ink == 0xf0c36a)
        #expect(rows[3].ways == "Rent, Buy")
    }

    @Test func justWatchLinksUseItsCountryNames() {
        #expect(WhereToWatch.justWatchLink(country: "GB", title: "Dune: Part Two")
            == "https://www.justwatch.com/uk/search?q=Dune%3A%20Part%20Two")
        #expect(WhereToWatch.justWatchLink(country: "AU", title: "Up") == "https://www.justwatch.com/au/search?q=Up")
    }

    @Test func theRegionComesFromAnyWayOfWritingALocale() {
        #expect(WhereToWatch.region(of: "en-AU") == "AU")
        #expect(WhereToWatch.region(of: "en_GB.UTF-8") == "GB")
        #expect(WhereToWatch.region(of: "zh-Hant-HK") == "HK")
        #expect(WhereToWatch.region(of: "en") == nil)
        #expect(WhereToWatch.region(of: "C.UTF-8") == nil)
    }

    @Test func theCountryIsPickedOrAutomatic() throws {
        let store = try Store.inMemory()
        #expect(WhereToWatch.settingsIndex(store) == 0)
        #expect(WhereToWatch.settingsList().first!.hasPrefix("Automatic ("))
        #expect(WhereToWatch.settingsList().count == 41)
        try WhereToWatch.choose(2, store: store) // Australia
        #expect(WhereToWatch.country(store) == "AU")
        #expect(try store.get(WhereToWatch.countryKey, as: String.self) == "AU")
        #expect(WhereToWatch.settingsIndex(store) == 2)
        try WhereToWatch.choose(0, store: store)
        #expect(WhereToWatch.chosenCountry(store) == nil)
        try store.put(WhereToWatch.countryKey, "XX") // not offered: ignored
        #expect(WhereToWatch.chosenCountry(store) == nil)
    }

    @Test func lookingUpFallsBackToOtherCountries() async {
        let http = HTTP(protocolClasses: [FixtureServer.self])
        var tmdb = TMDB(key: "k2", http: http)
        tmdb.api = "https://tmdb3.test/3"
        FixtureServer.answer("https://tmdb3.test/3/find/tt0000031?external_source=imdb_id&api_key=k2",
                             #"{"movie_results": [{"id": 31}]}"#)
        FixtureServer.answer("https://tmdb3.test/3/movie/31/watch/providers?api_key=k2",
                             #"{"results": {"GB": {"buy": [{"provider_name": "Shop"}]}}}"#)
        var meta = MetaDetail(id: "tt0000031", kind: "movie", name: "Thirty One")
        let found = await WhereToWatch.lookUp(meta, tmdb: tmdb, chosen: "AU")
        #expect(found.note == "United Kingdom (nothing in Australia)")
        #expect(found.link == "https://www.justwatch.com/uk/search?q=Thirty%20One")
        #expect(found.tiles.map(\.group) == ["Buy"])

        meta.id = "tt0000032" // unknown to the stand-in: TMDB answers 404
        let failed = await WhereToWatch.lookUp(meta, tmdb: tmdb, chosen: "AU")
        #expect(failed.note == "Could not look this up: TMDB answered with error 404.")
        #expect(failed.tiles.isEmpty)
    }
}
