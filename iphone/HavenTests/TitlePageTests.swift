import Foundation
import Testing
@testable import Haven

/// The Rust build's title page tests (`library/sources.rs`, `marks.rs`),
/// ported, and the title page's model.
struct TitleRulesTests {
    private func video(_ id: String, _ season: Int?, _ episode: Int?, _ released: String? = nil, _ title: String? = "Pilot") -> Video {
        Video(id: id, title: title, season: season, episode: episode, released: released)
    }

    @Test func sizesAreFoundInTheDetails() throws {
        let line = "Movie.2024.2160p 💾 12.3 GB 👤 50"
        let (size, range) = try #require(TitleRules.sizeIn(line))
        #expect(size == "12.3 GB")
        let chars = Array(line)
        #expect(TitleRules.tidy(String(chars[..<range.lowerBound]) + String(chars[range.upperBound...])) == "Movie.2024.2160p 👤 50")
        #expect(TitleRules.sizeIn("1080p · 850MB · WEB")?.0 == "850 MB")
        #expect(TitleRules.sizeIn("x 1,4 GiB")?.0 == "1.4 GB")
        #expect(TitleRules.sizeIn("H.264 5.1 Atmos 2160p") == nil)
        #expect(TitleRules.sizeIn("Show.S01E02.MBC") == nil)
    }

    @Test func howManyShareIsFoundInTheDetails() throws {
        let line = "BluRay REMUX 👥 418 🔍 Somewhere"
        let (count, range) = try #require(TitleRules.peersIn(line))
        #expect(count == "418")
        let chars = Array(line)
        #expect(TitleRules.tidy(String(chars[..<range.lowerBound]) + String(chars[range.upperBound...])) == "BluRay REMUX 🔍 Somewhere")
        #expect(TitleRules.peersIn("x 👤 1,204 y")?.0 == "1204")
        #expect(TitleRules.peersIn("1080p WEB-DL") == nil)
    }

    @Test func byteCountsReadWell() {
        #expect(TitleRules.sizeText(13_207_024_435) == "12.3 GB")
        #expect(TitleRules.sizeText(891_289_600) == "850 MB")
    }

    @Test func anEpisodeIsDescribedAsPlayingItRecords() {
        #expect(TitleRules.episodeDetail(video("x:1:2", 1, 2)) == "S1 E2 · Pilot")
        #expect(TitleRules.episodeDetail(video("x:2", nil, 2, nil, nil)) == "E2")
        let row = TitleRules.episodeRow(video("x:1:3", 1, 3, "2019-01-27T05:00:00.000Z", nil))
        #expect(row.number == "E3" && row.title == "Episode 3" && row.date == "2019-01-27")
        #expect(TitleRules.episodeRow(video("x", nil, nil, nil, nil)).title == "Untitled")
    }

    @Test func onlyAiredEpisodesCountAsOut() {
        #expect(TitleRules.aired(video("a", 1, 1, "2020-01-01T00:00:00.000Z"), today: "2026-11-22"))
        #expect(!TitleRules.aired(video("a", 1, 1, "2027-01-01"), today: "2026-11-22"))
        #expect(TitleRules.aired(video("a", 1, 1, nil), today: "2026-11-22"))
    }

    @Test func aStreamRowSplitsItsDetails() throws {
        let stream = MediaStream(name: "1080p\nWEB", description: "Some.Film.2024.1080p.WEB-DL · 💾 2.1 GB · 👥 12 · English · 5.1 · H.264 · from a long list of facts",
                                 url: "https://cdn.example/film.mkv")
        let row = try #require(TitleRules.streamRow(id: 1, source: "Home (Jellyfin)", stream))
        #expect(row.name == "1080p WEB")
        #expect(row.size == "2.1 GB" && row.peers == "12")
        #expect(row.detail == "Some.Film.2024.1080p.WEB-DL · English · 5.1")
        #expect(row.detail2 == "H.264 · from a long list of facts")
        #expect(row.kind == "Direct" && row.playable && row.group == "Home (Jellyfin)")
        // A size hint wins over the text; a page and a local file are not played.
        let hinted = try #require(TitleRules.streamRow(id: 2, source: "A", MediaStream(description: "3 GB", url: "https://x/y", videoSize: 891_289_600)))
        #expect(hinted.size == "850 MB" && hinted.detail.isEmpty)
        #expect(TitleRules.streamRow(id: 3, source: "A", MediaStream(externalURL: "https://x"))?.kind == "Web page")
        #expect(TitleRules.streamRow(id: 4, source: "A", MediaStream(url: "file:///etc/hosts"))?.playable == false)
        #expect(TitleRules.streamRow(id: 5, source: "A", MediaStream(infoHash: "abc"))?.kind == "Peer to peer")
        #expect(TitleRules.streamRow(id: 6, source: "A", MediaStream(name: "Nothing")) == nil)
        #expect(TitleRules.streamRow(id: 7, source: "A", MediaStream(url: "https://x", provider: "Gathered"))?.group == "Gathered")
    }

    @Test func eachProviderShowsTenAtATimeUnderItsHeading() {
        func row(_ id: Int, _ group: String) -> StreamRow {
            StreamRow(id: id, source: group, name: "\(id)", detail: "", detail2: "", size: "", peers: "", kind: "Direct", group: group)
        }
        let rows = (0 ..< 12).map { row($0, "A") } + [row(12, "B")]
        let marked = TitleRules.marked(rows, limits: [:])
        #expect(marked[0].heading == "A  ·  12" && marked[1].heading.isEmpty)
        #expect(marked[9].more == 2 && marked[9].shown && !marked[10].shown)
        #expect(marked[12].heading == "B  ·  1" && marked[12].more == 0)
        let more = TitleRules.marked(rows, limits: ["A": 20])
        #expect(more.filter(\.shown).count == 13 && more.allSatisfy { $0.more == 0 })
        #expect(TitleRules.groups(rows).map(\.name) == ["A", "B"] && TitleRules.groups(rows)[0].count == 12)
    }

    @Test func serversStayAboveAndProvidersStayTogether() {
        func row(_ group: String) -> StreamRow {
            StreamRow(id: 0, source: group, name: "", detail: "", detail2: "", size: "", peers: "", kind: "Direct", group: group)
        }
        let rows = [row("Server"), row("Add-on A"), row("Add-on B")]
        #expect(TitleRules.place(row("Server"), in: rows, serverRows: 1, fromServer: true) == 1)
        #expect(TitleRules.place(row("Other server"), in: rows, serverRows: 1, fromServer: true) == 1)
        #expect(TitleRules.place(row("Add-on A"), in: rows, serverRows: 1, fromServer: false) == 2)
        #expect(TitleRules.place(row("Add-on C"), in: rows, serverRows: 1, fromServer: false) == 3)
    }

    @Test func theNextEpisodeSkipsSpecials() {
        var meta = MetaDetail(id: "tt1", kind: "series", name: "Show")
        meta.videos = [video("tt1:1:2", 1, 2), video("tt1:0:1", 0, 1), video("tt1:2:1", 2, 1), video("tt1:1:1", 1, 1)]
        #expect(TitleRules.nextVideo(meta, after: "tt1:1:1")?.id == "tt1:1:2")
        #expect(TitleRules.nextVideo(meta, after: "tt1:1:2")?.id == "tt1:2:1")
        #expect(TitleRules.nextVideo(meta, after: "tt1:2:1") == nil)
    }

    @Test func addOnsAreAskedByTheIMDbForm() {
        var meta = MetaDetail(id: "srv:1:abc", kind: "series", name: "Show")
        meta.imdbID = "tt0944947"
        meta.releaseInfo = "2011"
        meta.videos = [video("srv:1:ep", 3, 4)]
        let wanted = TitleRules.wanted(kind: "series", videoID: "srv:1:ep", openID: "srv:1:abc", meta: meta, name: "")
        #expect(wanted.imdb == "tt0944947" && wanted.year == "2011" && wanted.episode?.season == 3 && wanted.episode?.number == 4)
        #expect(TitleRules.addonID(videoID: "srv:1:ep", wanted: wanted) == "tt0944947:3:4")
        #expect(TitleRules.addonID(videoID: "tt0944947:3:4", wanted: wanted) == "tt0944947:3:4", "an add-on's own id")
        let film = TitleRules.wanted(kind: "movie", videoID: "tmdb:movie:603", openID: "tmdb:movie:603", meta: nil, name: "The Matrix")
        #expect(film.tmdb == "603" && film.imdb == nil && film.name == "The Matrix")
        #expect(TitleRules.addonID(videoID: "tmdb:movie:603", wanted: film) == nil)
        #expect(TitleRules.wanted(kind: "series", videoID: "tt0944947:1:1", openID: "x", meta: nil, name: "S").imdb == "tt0944947")
    }

    @Test func anIMDbIdIsFoundByNameAndYear() {
        let found = [MetaPreview(id: "tt1", kind: "movie", name: "Dune", releaseInfo: "1984"),
                     MetaPreview(id: "kitsu:2", kind: "movie", name: "Dune: Part Two", releaseInfo: "2024"),
                     MetaPreview(id: "tt1160419", kind: "movie", name: "Dune", releaseInfo: "2021")]
        #expect(TitleRules.imdbMatch(found, name: "DUNE", year: "2021") == "tt1160419")
        #expect(TitleRules.imdbMatch(found, name: "Dune", year: nil) == "tt1")
        #expect(TitleRules.imdbMatch(found, name: "Dune Part Two", year: "2024") == nil, "not an IMDb id")
        #expect(TitleRules.imdbMatch(found, name: "  ", year: nil) == nil)
    }
}

@MainActor
struct TitlePageTests {
    private func library() throws -> Library {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("haven-\(UUID().uuidString)")
        return Library(store: try Store.inMemory(), keys: Secrets(), http: HTTP(protocolClasses: [FixtureServer.self]), kept: Kept(dir: folder))
    }

    private func wait(_ done: () -> Bool) async throws {
        for _ in 0 ..< 100 where !done() { try await Task.sleep(for: .milliseconds(30)) }
    }

    private static let series = """
        {"meta": {"id": "tt7700001", "type": "series", "name": "A Show", "releaseInfo": "2020–", "imdbRating": "8.1",
                  "genres": ["Drama", "Crime", "Mystery", "Thriller"], "description": "  Things happen. ", "cast": ["A", "B"],
                  "videos": [{"id": "tt7700001:1:1", "title": "One", "season": 1, "episode": 1, "released": "2020-01-01T00:00:00.000Z"},
                             {"id": "tt7700001:1:2", "title": "Two", "season": 1, "episode": 2, "released": "2020-01-08T00:00:00.000Z"},
                             {"id": "tt7700001:1:3", "title": "Three", "season": 1, "episode": 3, "released": "2099-01-01T00:00:00.000Z"},
                             {"id": "tt7700001:2:1", "title": "Again", "season": 2, "episode": 1, "released": "2021-01-01T00:00:00.000Z"},
                             {"id": "tt7700001:0:1", "title": "Extra", "season": 0, "episode": 1}]}}
        """

    @Test func aSeriesOpensOnItsSeasonsAndMarksAiredEpisodes() async throws {
        FixtureServer.answer(try ResourceURL.make(Cinemeta.url, resource: "meta", kind: "series", id: "tt7700001"), Self.series)
        let library = try library()
        let page = library.title!
        page.open(Opening(id: "tt7700001", kind: "series", title: "A Show"))
        #expect(page.loading && page.title == "A Show")
        try await wait { !page.loading }
        #expect(page.facts == "2020–  ·  ★ 8.1  ·  Drama, Crime, Mystery")
        #expect(page.description == "Things happen.")
        #expect(page.cast == "A, B")
        #expect(page.seasons == [1, 2, 0], "specials last")
        #expect(page.season == 1 && page.episodes.map(\.title) == ["One", "Two", "Three"])
        #expect(page.isSeries && !page.sourcesShown)
        #expect(page.sources.note == TitleRules.chooseEpisode)
        #expect(page.offersNote == WhereToWatch.noKeyNote)

        page.markSeason(true)
        #expect(page.episodes.map(\.watched) == [true, true, false], "the one not out yet stays unwatched")
        #expect(!page.seasonAllWatched)
        page.markEpisodes(["tt7700001:1:3"], on: true)
        #expect(page.seasonAllWatched)
        #expect(library.marks > 0)
        let saved = try library.store.progress(kind: "series", titleID: "tt7700001", videoID: "tt7700001:1:2")
        #expect(saved?.finished == true && saved?.detail == "S1 E2 · Two" && saved?.title == "A Show")
        page.markSeason(false)
        #expect(page.episodes.allSatisfy { !$0.watched })

        page.episodeFilter = "tw"
        #expect(page.episodes.map(\.title) == ["Two"])
        page.episodeFilter = ""
        page.openEpisode("tt7700001:1:2")
        #expect(page.episodeOpen && page.panelOpen && page.sourcesShown)
        #expect(page.episodeLabel == "S1 E2 · Two")
        #expect(page.sources.listed == SourcesPanel.Listed(videoID: "tt7700001:1:2", detail: "S1 E2 · Two"))
        #expect(page.sources.note == TitleRules.nothingCanPlay, "no server, add-on or account to ask")
        #expect(page.episodes.first { $0.selected }?.id == "tt7700001:1:2")

        // Opened again: the kept details show at once.
        page.close()
        page.open(Opening(id: "tt7700001", kind: "series", title: "A Show"))
        #expect(page.seasons == [1, 2, 0] && page.facts.hasPrefix("2020–"))
    }

    @Test func aContinueWatchingCardOpensItsEpisode() async throws {
        FixtureServer.answer(try ResourceURL.make(Cinemeta.url, resource: "meta", kind: "series", id: "tt7700001"), Self.series)
        let library = try library()
        let page = library.title!
        page.open(Opening(id: "tt7700001", kind: "series", title: "A Show", video: "tt7700001:2:1"))
        try await wait { !page.loading }
        #expect(page.season == 2 && page.episodeOpen && page.episodeLabel == "S2 E1 · Again")
    }

    @Test func aFilmIsMarkedAndSavedToTheWatchlist() async throws {
        FixtureServer.answer(try ResourceURL.make(Cinemeta.url, resource: "meta", kind: "movie", id: "tt7700002"),
                             #"{"meta": {"id": "tt7700002", "type": "movie", "name": "A Film", "releaseInfo": "2024", "runtime": "101 min", "poster": "https://p/f.jpg"}}"#)
        let library = try library()
        var queued: [String] = []
        library.queueSync = { what, _, _, _ in queued.append(what) }
        let page = library.title!
        page.open(Opening(id: "tt7700002", kind: "movie", title: "A Film"))
        try await wait { !page.loading }
        #expect(page.film && !page.isSeries && page.sourcesShown)
        #expect(page.facts == "2024  ·  101 min")
        #expect(page.poster == "https://p/f.jpg")

        page.markFilm(true)
        #expect(page.watched && library.filmWatched(kind: "movie", id: "tt7700002"))
        page.showFilmSources()
        #expect(page.resumeNote == "You have watched this.")
        page.markFilm(false)
        #expect(!page.watched && !library.filmWatched(kind: "movie", id: "tt7700002"))
        #expect(queued == ["watched", "unwatched"])

        page.toggleWatchlist()
        #expect(page.saved && library.watchlist.map(\.title) == ["A Film"])
        #expect(library.watchlist.first?.year == "2024")
        page.toggleWatchlist()
        #expect(!page.saved && library.watchlist.isEmpty)
        #expect(queued.suffix(2) == ["watchlist_add", "watchlist_remove"])

        // A film part watched says where it resumes.
        try library.store.saveProgress(WatchProgress(kind: "movie", titleID: "tt7700002", videoID: "tt7700002", title: "A Film", detail: "",
                                                     poster: nil, position: 600, duration: 6000, finished: false, updatedAt: 1))
        page.showResumeNote()
        #expect(page.resumeNote.hasPrefix("Resumes at "))
        // Nothing plays until Haven's player comes.
        page.play(StreamRow(id: 0, source: "S", name: "n", detail: "", detail2: "", size: "", peers: "", kind: "Direct",
                            address: "https://x/f.mp4", group: "S"))
        #expect(!page.playNote.isEmpty)
    }

    @Test func aTitleNothingKnowsSaysSo() async throws {
        let library = try library()
        let page = library.title!
        page.open(Opening(id: "tmdb:movie:1", kind: "movie", title: "Gone"))
        #expect(page.error == TitleRules.noTMDBKey && !page.loading)
        page.open(Opening(id: "kitsu:1", kind: "series", title: "Gone"))
        #expect(page.error == TitleRules.noDetails)
    }
}
