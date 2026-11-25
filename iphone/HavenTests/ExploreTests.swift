import Foundation
import Testing
@testable import Haven

/// Search (the Rust build's `run_search`) and Explore (`library/explore.rs`).
@MainActor
struct ExploreTests {
    private func library(key: String? = nil, kept: Kept? = nil) throws -> Library {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("haven-\(UUID().uuidString)")
        return Library(store: try Store.inMemory(), keys: Secrets(tmdbAPIKey: key),
                       http: HTTP(protocolClasses: [FixtureServer.self]), kept: kept ?? Kept(dir: folder))
    }

    /// A TMDB page of films numbered `from` through `to`.
    private static func films(_ from: Int, _ to: Int) -> String {
        let results = (from ... to).map { #"{"id": \#($0), "title": "Film \#($0)", "poster_path": "/p\#($0).jpg"}"# }
        return #"{"results": [\#(results.joined(separator: ","))]}"#
    }

    private static func discover(_ key: String, page: Int, genre: Int? = nil) -> String {
        "\(TMDB.api)/discover/movie?sort_by=popularity.desc&page=\(page)" + (genre.map { "&with_genres=\($0)" } ?? "") + "&api_key=\(key)"
    }

    private func wait(_ done: () -> Bool) async throws {
        for _ in 0 ..< 100 where !done() { try await Task.sleep(for: .milliseconds(30)) }
    }

    // MARK: Search

    @Test func searchNotesSayWhyNothingCanBeSearched() {
        #expect(Library.searchNote(sources: 1, plans: 2) == "")
        #if ADDONS
        #expect(Library.searchNote(sources: 0, plans: 0) == "Add a media server or a TMDB key in Settings, or install an add-on: search looks through those.")
        #expect(Library.searchNote(sources: 1, plans: 0) == "None of your add-ons can be searched.")
        #else
        #expect(Library.searchNote(sources: 0, plans: 0) == "Add a media server or a TMDB key in Settings: search looks through those.")
        #expect(Library.searchNote(sources: 1, plans: 0) == "Nothing you have added can be searched.")
        #endif
    }

    @Test func searchAsksEverySearchableCatalogueAndTMDB() throws {
        let library = try library(key: "search-key")
        library.runSearch("  dune ")
        // Cinemeta's two searchable catalogues, then TMDB's films and series.
        #expect(library.searchBoard.shelves.map(\.title) == ["Films", "Series", "Films", "Series"])
        #expect(library.searchBoard.shelves.map(\.source) == ["Cinemeta", "Cinemeta", "TMDB", "TMDB"])
        #expect(library.searchBoard.shelves.allSatisfy { $0.keptName.isEmpty }, "search results are not kept")
        #expect(library.searchNote == "")
        library.runSearch("   ")
        #expect(library.searchBoard.shelves.isEmpty)
    }

    // MARK: Explore

    @Test func sourcesAreListedUnderWhereTheyComeFrom() throws {
        let library = try library(key: "rows-key")
        let explore = library.explore!
        explore.rebuild()
        #expect(explore.sources.first == .tmdb)
        #expect(explore.sources.count == 5, "TMDB and Cinemeta's four catalogues")
        #expect(explore.rows.filter(\.header).map(\.label) == ["TMDB", "Cinemeta"])
        #expect(explore.source == 0 && explore.kinds == ["Films", "Series"])
        explore.words = "series cinemeta"
        #expect(explore.rows.first?.label == "Cinemeta" && explore.rows.count == 3)
        explore.words = "nothing like it"
        #expect(explore.rows.isEmpty)
        // No TMDB key: TMDB is no longer a source, and the first one is chosen.
        library.setKeys(Secrets())
        #expect(explore.sources.count == 4)
        #expect(explore.source == 0 && explore.kinds.isEmpty)
    }

    @Test func tmdbPagesComeTwoAtATimeWithoutRepeats() async throws {
        let key = "pages-key"
        FixtureServer.answer("\(TMDB.api)/genre/movie/list?api_key=\(key)", #"{"genres": [{"id": 28, "name": "Action"}]}"#)
        FixtureServer.answer(Self.discover(key, page: 1), Self.films(1, 20))
        FixtureServer.answer(Self.discover(key, page: 2), Self.films(21, 40))
        FixtureServer.answer(Self.discover(key, page: 3), Self.films(39, 41))
        let library = try library(key: key)
        let explore = library.explore!
        explore.rebuild()
        try await wait { !explore.genres.isEmpty }
        #expect(explore.genres == ["All genres", "Action"])
        #expect(FixtureServer.count(Self.discover(key, page: 1)) == 0, "nothing is fetched before Explore is opened")
        explore.appeared()
        try await wait { explore.items.count == 40 }
        #expect(explore.items.count == 40 && explore.more && explore.note == "")
        #expect(explore.items[0].id == "tmdb:movie:1")
        explore.load()
        try await wait { !explore.loading }
        // Page 3 repeats two films; page 4 fails but page 3 came.
        #expect(explore.items.count == 41)
        #expect(!explore.more, "fewer than two full pages: the end")

        // The next start shows the first two pages at once, kept.
        let again = try self.library(key: key, kept: library.kept)
        again.explore.rebuild()
        #expect(again.explore.items.count == 40)

        // A genre that cannot be loaded says so.
        FixtureServer.answer(Self.discover(key, page: 1, genre: 28), status: 500, "{}")
        explore.chooseGenre(1)
        #expect(explore.genre == 0 && explore.items.isEmpty)
        try await wait { !explore.note.isEmpty }
        #expect(explore.note.hasPrefix("Could not load this: "))
        #expect(!explore.more)
    }

    @Test func aRefreshKeepsTheTitlesUntilTheFreshOnesCome() async throws {
        let key = "refresh-key"
        FixtureServer.answer(Self.discover(key, page: 1), Self.films(1, 20))
        let library = try library(key: key)
        let explore = library.explore!
        explore.rebuild()
        explore.appeared()
        try await wait { explore.items.count == 20 }
        #expect(!explore.more)
        FixtureServer.answer(Self.discover(key, page: 1), Self.films(5, 24))
        await explore.refresh()
        #expect(explore.items.map(\.id).first == "tmdb:movie:5" && explore.items.count == 20)
        #expect(!explore.refreshing)
    }

    @Test func titlesAreAddedOnce() {
        func title(_ id: String, _ name: String, _ year: String? = nil) -> MetaPreview {
            MetaPreview(id: id, kind: "movie", name: name, releaseInfo: year)
        }
        let (list, added) = Explore.adding([title("tt2", "Two"), title("tt1", "One"), title("tt9", "One"), title("tt3", "Three", "2020"),
                                            title("tt4", "Three", "2021")],
                                           to: [title("tt1", "One")])
        #expect(list.map(\.id) == ["tt1", "tt2", "tt3", "tt4"], "the same id, or the same name and year, is a repeat")
        #expect(added == 3)
        #expect(Explore.choices([]) == [] && Explore.choices(["Drama"]) == ["All genres", "Drama"])
    }

    @Test func aSourceIsLabelledByNameAndOrigin() {
        let server = Server(kind: .jellyfin, name: "Home", address: "http://192.168.1.2:8096", token: "t")
        #expect(ExploreSource.tmdb.label == "Films and series  ·  TMDB")
        #expect(ExploreSource.library(server, MediaLibrary(id: "1", name: "Films", collection: false)).label == "Films  ·  Home")
    }
}
