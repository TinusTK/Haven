import Foundation
import Testing
@testable import Haven

private func meta(_ json: String) -> MetaDetail {
    try! Titles.parseMeta(#"{"meta": \#(json)}"#)
}

/// The Rust build's calendar tests (release dates, the month page), ported.
struct ReleasesTests {
    private let utc = TimeZone(identifier: "UTC")!

    @Test func aMomentMovesToTheLocalDayAndABareDateStays() {
        let sydney = TimeZone(secondsFromGMT: 11 * 3600)!
        let newYork = TimeZone(secondsFromGMT: -5 * 3600)!
        // 9 pm on the 11th in New York is 02:00 UTC on the 12th.
        let aired = "2026-11-12T02:00:00.000Z"
        #expect(Releases.local(aired, zone: newYork)! == (Day(year: 2026, month: 11, day: 11), Clock(hour: 21, minute: 0)))
        #expect(Releases.local(aired, zone: sydney)! == (Day(year: 2026, month: 11, day: 12), Clock(hour: 13, minute: 0)))
        // The same moment written with its own offset.
        #expect(Releases.local("2026-11-11T21:00:00-05:00", zone: sydney)! == (Day(year: 2026, month: 11, day: 12), Clock(hour: 13, minute: 0)))
        // No time known: the date as given, wherever you are.
        #expect(Releases.local("2026-11-11", zone: sydney)! == (Day(year: 2026, month: 11, day: 11), nil))
        #expect(Releases.local("2026-11-11T00:00:00.000Z", zone: newYork)! == (Day(year: 2026, month: 11, day: 11), nil))
        #expect(Releases.local("soon", zone: sydney) == nil)
    }

    @Test func aMonthPageIsSixWeeksFromAMonday() {
        let days = Releases.month(year: 2026, month: 11)
        #expect(days.count == 42)
        // 1 November 2026 is a Sunday: the page starts Monday 26 October.
        #expect(days[0] == Day(year: 2026, month: 10, day: 26))
        #expect(days[6] == Day(year: 2026, month: 11, day: 1))
        #expect(days[41] == Day(year: 2026, month: 12, day: 6))
        #expect(Releases.week(of: Day(year: 2026, month: 11, day: 19))[0] == Day(year: 2026, month: 11, day: 16))
    }

    @Test func episodesAndFilmsGiveReleases() {
        let series = meta(#"{"id": "tt1", "type": "series", "name": "A Show", "released": "2020-01-01", "videos": [{"id": "tt1:3:5", "title": "Five", "season": 3, "episode": 5, "released": "2026-11-18T08:30:00.000Z"}, {"id": "tt1:3:6", "season": 3, "episode": 6}]}"#)
        let releases = Releases.of(series, zone: utc)
        #expect(releases.count == 1)
        #expect(releases[0].label == "S3 E5 · Five")
        #expect(releases[0].video == "tt1:3:5")
        #expect(releases[0].time == Clock(hour: 8, minute: 30))
        #expect(releases[0].number == "S03E05")
        let doubled = meta(#"{"id": "tt3", "type": "series", "name": "B", "videos": [{"id": "tt3:0:31", "title": "The Making Of Episode 1", "season": 0, "episode": 31, "released": "2026-11-11T11:00:00.000Z"}, {"id": "tt3:3:1", "title": "One", "season": 3, "episode": 1, "released": "2026-11-11T11:00:00.000Z"}, {"id": "tt3:3:1", "title": "One", "season": 3, "episode": 1, "released": "2026-11-11T11:00:00.000Z"}]}"#)
        let once = Releases.of(doubled, zone: utc)
        #expect(once.count == 1)
        #expect(once[0].label == "S3 E1 · One")
        let film = meta(#"{"id": "tt2", "type": "movie", "name": "A Film", "released": "2026-12-25T00:00:00.000Z"}"#)
        let filmReleases = Releases.of(film, zone: utc)
        #expect(filmReleases.count == 1)
        #expect(filmReleases[0].day == Day(year: 2026, month: 12, day: 25) && filmReleases[0].time == nil && filmReleases[0].label == "")
    }

    @Test func upcomingShowsEachShowOnceFromToday() {
        let today = Day(year: 2026, month: 11, day: 25)
        func release(_ day: Int, _ number: String, _ name: String = "", month: Int = 11) -> Release {
            Release(day: Day(year: 2026, month: month, day: day), time: nil, label: "", video: "v\(number)", number: number, name: name)
        }
        let show = Releases.Title(kind: "series", id: "tt1")
        let other = Releases.Title(kind: "series", id: "tt2")
        let film = Releases.Title(kind: "movie", id: "tt3")
        let cards = Releases.upcoming([
            show: [release(24, "S03E06"), release(26, "S03E07", "Seven"), release(26, "S03E08"), release(30, "S03E09")],
            other: [release(25, "S01E01", "Pilot")],
            film: [release(10, "", month: 12)],
        ], names: [show: "A Show", other: "Other", film: "A Film"], pictures: [show: "https://p/1.jpg"], today: today)
        #expect(cards.map(\.title) == ["Other", "A Show", "A Film"])
        #expect(cards.map(\.when) == ["Today", "Tomorrow", "Dec 10"])
        #expect(cards[1].episode == "New episodes" && cards[1].badge == "S03E07–E08")
        #expect(cards[0].episode == "Pilot" && cards[0].soon)
        #expect(cards[2].episode == "Film" && !cards[2].soon)
        #expect(cards[1].picture == "https://p/1.jpg")
    }

    @Test func releaseDayCountsDownByTheHour() {
        let zone = TimeZone(identifier: "Australia/Adelaide")!
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = zone
        func at(_ day: Int, _ hour: Int, _ minute: Int) -> Date {
            calendar.date(from: DateComponents(year: 2026, month: 11, day: day, hour: hour, minute: minute))!
        }
        let day = Day(year: 2026, month: 11, day: 25)
        let nine = Clock(hour: 21, minute: 0)
        #expect(Releases.whenLabel(day: day, time: nine, now: at(25, 16, 10), zone: zone) == "In 4 h")
        #expect(Releases.whenLabel(day: day, time: nine, now: at(25, 20, 15), zone: zone) == "In 45 min")
        #expect(Releases.whenLabel(day: day, time: nine, now: at(25, 21, 0), zone: zone) == "Out now")
        #expect(Releases.whenLabel(day: day, time: nil, now: at(25, 9, 0), zone: zone) == "Today")
        #expect(Releases.whenLabel(day: day, time: nine, now: at(24, 23, 0), zone: zone) == "Tomorrow")
        #expect(Releases.whenLabel(day: day, time: nil, now: at(19, 9, 0), zone: zone) == "In 6 days")
        #expect(Releases.whenLabel(day: day, time: nil, now: at(18, 9, 0), zone: zone) == "Nov 25")
    }
}

/// The Rust build's Home tests (home.rs), ported, and Home's model.
struct HomeTopTests {
    @Test func episodesLeftCountFromTheCurrentOne() {
        func video(_ s: Int, _ e: Int, _ day: String) -> Video {
            Video(id: "tt1:\(s):\(e)", season: s, episode: e, released: day)
        }
        let videos = [video(0, 1, "2020-01-01"), video(1, 1, "2020-01-01"), video(1, 2, "2020-01-08"),
                      video(1, 3, "2020-01-15"), video(2, 1, "2030-01-01")]
        #expect(HomeTop.episodesLeft(videos, current: "tt1:1:1", finished: ["tt1:1:3"], today: "2026-11-19") == 2)
        #expect(HomeTop.episodesLeft(videos, current: "tt9:1:1", finished: ["tt1:1:3"], today: "2026-11-19") == 0)
        #expect(HomeTop.episodeBadge("tt1:3:6") == "S03E06")
        #expect(HomeTop.episodeBadge("tt1") == "")
    }

    @Test func theBannerSaysWhatIsKnown() {
        var film = MetaDetail(id: "tt1", kind: "movie", name: "A Film")
        film.releaseInfo = "2026"
        film.genres = ["Adventure", "Action", "Fantasy", "Drama"]
        film.rating = "8.2"
        let scores = OMDb.Scores(imdb: "8.0", rottenTomatoes: "94%", metacritic: "78", rated: "R", runtime: "173 min")
        #expect(HomeTop.bannerFacts(film, scores: scores) == "2026  ·  Adventure, Action, Fantasy  ·  173 min  ·  R")
        #expect(HomeTop.bannerFacts(film, scores: nil) == "2026  ·  Adventure, Action, Fantasy")
        let pills = HomeTop.scorePills(film, tmdb: 7.84, scores: scores)
        #expect(pills.map { "\($0.label) \($0.value)" } == ["IMDb 8.2", "TMDB 7.8", "RT 94%", "MC 78"])
        #expect(HomeTop.scorePills(film, tmdb: 0, scores: nil).map(\.label) == ["IMDb"])
    }

    @Test func continueCardsNameTheEpisodeAndWhatIsLeft() {
        let progress = WatchProgress(kind: "series", titleID: "tt1", videoID: "tt1:1:2", title: "A Show", detail: "S1 E2 · Second",
                                     poster: "https://p/poster.jpg", position: 600, duration: 3000, finished: false, updatedAt: 1)
        let film = WatchProgress(kind: "movie", titleID: "tt5", videoID: "tt5", title: "A Film", detail: "",
                                 poster: nil, position: 10, duration: 6000, finished: false, updatedAt: 2)
        let key = Releases.Title(kind: "series", id: "tt1")
        let videos = (1 ... 4).map { (n: Int) in Video(id: "tt1:1:\(n)", season: 1, episode: n, released: "2020-01-0\(n)") }
        let cards = HomeTop.cards([progress, film], known: [key: ("https://p/wide.jpg", videos)], finished: { _ in ["tt1:1:3"] }, today: "2026-01-01")
        #expect(cards[0].episode == "Second" && cards[0].badge == "S01E02" && cards[0].left == 2)
        #expect(cards[0].picture == "https://p/wide.jpg")
        #expect(abs(cards[0].progress - 0.2) < 0.0001)
        #expect(cards[1].episode == "" && cards[1].badge == "" && cards[1].progress == 0.02, "a bar is always seen")
    }
}

@MainActor
struct LibraryTests {
    private func library(_ name: String) throws -> (Library, Store) {
        let store = try Store.inMemory()
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("haven-\(UUID().uuidString)")
        let library = Library(store: store, keys: Secrets(), http: HTTP(protocolClasses: [FixtureServer.self]), kept: Kept(dir: folder))
        return (library, store)
    }

    private static let popular = """
        {"metas": [{"id": "tt0000101", "type": "movie", "name": "One", "imdbRating": "7.1"},
                   {"id": "tt0000102", "type": "movie", "name": "Two"},
                   {"id": "tt0000103", "type": "movie", "name": "Three"},
                   {"id": "tt0000102", "type": "movie", "name": "Two"}]}
        """

    @Test func homeFillsFromCinemetaAndKeepsItForTheNextStart() async throws {
        let (library, _) = try library("h1")
        for catalog in Cinemeta.fallback {
            FixtureServer.answer(try ResourceURL.catalog(Cinemeta.url, catalog), Self.popular)
        }
        library.reloadHome()
        #expect(library.homeBoard.shelves.map(\.title) == ["Popular films", "Popular series", "Featured films", "Featured series"])
        #expect(library.homeBoard.shelves.allSatisfy { $0.loading })
        for _ in 0 ..< 50 where library.homeBoard.settled < 4 { try await Task.sleep(for: .milliseconds(50)) }
        let first = library.homeBoard.shelves[0]
        #expect(!first.loading && first.items.map(\.name) == ["One", "Two", "Three"], "repeats are dropped")
        // The banner takes the first shelf with 3 titles.
        #expect(library.hero.map(\.title) == ["One", "Two", "Three"])
        // The next start shows the kept titles at once.
        let again = Library(store: try Store.inMemory(), keys: Secrets(), http: HTTP(protocolClasses: [FixtureServer.self]), kept: library.kept)
        again.homeShowing(false)
        again.reloadHome()
        #expect(again.homeBoard.shelves[0].items.map(\.name) == ["One", "Two", "Three"])
        #expect(!again.homeBoard.shelves[0].loading)
    }

    @Test func aShelfThatFailsSaysSo() async throws {
        let (library, _) = try library("h2")
        library.fill(library.homeBoard, [ShelfPlan(title: "Broken films", source: "Nowhere",
                                                   from: .catalogue(url: "https://nowhere.test/manifest.json", catalog: Catalog(kind: "movie", id: "x")))])
        for _ in 0 ..< 50 where library.homeBoard.settled < 1 { try await Task.sleep(for: .milliseconds(50)) }
        #expect(library.homeBoard.shelves[0].error == "Could not load this: the server answered with error 404.")
        // No TMDB key: no TMDB shelves.
        library.reloadCatalog()
        #expect(library.catalogBoard.shelves.isEmpty)
    }

    @Test func continueWatchingAndTheWatchlistComeFromTheDatabase() throws {
        let (library, store) = try library("h3")
        try store.saveProgress(WatchProgress(kind: "movie", titleID: "tt7", videoID: "tt7", title: "Seven", detail: "", poster: "https://p/7.jpg",
                                             position: 900, duration: 3600, finished: false, updatedAt: 5))
        try store.addToWatchlist(Saved(kind: "movie", titleID: "tt8", title: "Eight", year: "2026", poster: "https://p/8.jpg", addedAt: 1))
        library.reloadContinue()
        library.reloadWatchlist()
        #expect(library.continueCards.map(\.title) == ["Seven"])
        #expect(library.continueCards[0].picture == "https://p/7.jpg")
        #expect(library.watchlist.map(\.title) == ["Eight"])
        library.removeContinue(kind: "movie", id: "tt7")
        #expect(library.continueCards.isEmpty)
        #expect(!library.filmWatched(kind: "movie", id: "tt7"))
        try store.saveProgress(WatchProgress(kind: "movie", titleID: "tt9", videoID: "tt9", title: "Nine", detail: "", poster: nil,
                                             position: 0, duration: 0, finished: true, updatedAt: 6))
        #expect(library.filmWatched(kind: "movie", id: "tt9"))
        #expect(!library.filmWatched(kind: "series", id: "tt9"))
    }

    @Test func theLoadingScreenWaitsForHomesShelves() async throws {
        let (library, _) = try library("h4")
        #expect(library.startupProgress(homeShowing: true) == 0, "Cinemeta is a source: its shelves are to come")
        library.reloadHome()
        #expect(library.startupProgress(homeShowing: true) < 1)
        #expect(library.startupProgress(homeShowing: false) == 1, "another page shows: Home is not waited for")
    }
}
