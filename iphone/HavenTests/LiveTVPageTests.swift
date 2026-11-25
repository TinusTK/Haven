import Foundation
import Testing
@testable import Haven

/// The Live TV page's model, and radio going on in the background.
@MainActor
struct LiveTVPageTests {
    private static let playlist = """
        #EXTM3U
        #EXTINF:-1 group-title="News",One News
        https://tv.example/one.m3u8
        #EXTINF:-1 group-title="Sport",Two Sport
        https://tv.example/two.m3u8
        #EXTINF:-1 group-title="Sport",Three Sport
        https://tv.example/three.m3u8
        #EXTINF:-1 radio="true" group-title="Music",Jazz FM
        https://radio.example/jazz.mp3
        #EXTINF:-1,Local file
        file:///etc/hosts
        """

    private func model() throws -> (LiveTVModel, Store) {
        let store = try Store.inMemory()
        try store.saveTvSource(TvSource(url: "https://tv.example/list.m3u", name: "tv.example", playlist: Self.playlist))
        let live = LiveTVModel(store: store)
        live.rebuild()
        return (live, store)
    }

    @Test func channelsAndStationsAreApartAndFiltered() throws {
        let (live, _) = try model()
        #expect(live.tv.channels.map(\.name) == ["One News", "Two Sport", "Three Sport"], "a file is never played")
        #expect(live.categories.map(\.label) == ["All channels", "News", "Sport"])
        #expect(live.categories.map(\.count) == [3, 1, 2])
        #expect(live.countText == "3 in all" && live.categoryName == "All channels")
        live.choose("g:Sport")
        #expect(live.channels.map(\.name) == ["Two Sport", "Three Sport"])
        #expect(live.countText == "2 of 3" && live.categoryName == "Sport")
        live.setWords("three")
        #expect(live.channels.map(\.name) == ["Three Sport"])
        live.setCategoryWords("new")
        #expect(live.categories.map(\.label) == ["All channels", "News"])
        live.band = 1
        #expect(live.isRadio && live.channels.map(\.name) == ["Jazz FM"])
        #expect(live.categories.first?.label == "All stations")
    }

    @Test func favouritesComeFirstAndAreKept() throws {
        let (live, store) = try model()
        live.chooseFavourites(1)
        #expect(live.choosing?.name == "Two Sport")
        live.newGroup("  Mine ")
        #expect(live.favourites.map(\.name) == ["Mine"] && live.has(live.favourites[0]))
        live.closeChooser()
        #expect(live.categories.first?.label == "Mine" && live.categories.first?.favourite == true)
        #expect(live.channels.first { $0.name == "Two Sport" }?.starred == true)
        live.choose("f:Mine")
        #expect(live.channels.map(\.name) == ["Two Sport"])
        // Radio lists only the groups that hold stations.
        live.band = 1
        #expect(live.categories.map(\.label) == ["All stations", "Music"])
        live.band = 0
        // Kept for the next start, as the Rust build kept them.
        #expect(LiveTV.favourites(store).map(\.channels) == [["https://tv.example/two.m3u8"]])
        live.chooseFavourites(1)
        live.toggle("Mine")
        #expect(live.channels.isEmpty, "the group is empty now")
        live.deleteGroup("Mine")
        #expect(live.board.category == LiveTV.all, "a chosen group that is gone falls back to every channel")
    }

    @Test func aSwitchedOffSourceIsNotShown() throws {
        let (live, store) = try model()
        _ = LiveTV.setEnabled("https://tv.example/list.m3u", on: false, store: store)
        live.rebuild()
        #expect(live.tv.channels.isEmpty && live.radio.channels.isEmpty)
    }

    @Test func aChannelPlaysWithNoPlaceKept() throws {
        let (live, _) = try model()
        var played: [PlayRequest] = []
        live.play = { played.append($0) }
        live.band = 1
        live.play(0)
        #expect(played.first?.kind == "radio" && played.first?.title == "Jazz FM" && played.first?.detail == "Music")
        #expect(played.first?.remember == false)
        live.play(9)
        #expect(played.count == 1)
    }

    @Test func radioGoesOnWhenHavenIsLeftAndFilmsWait() throws {
        let store = try Store.inMemory()
        let player = PlayerModel(store: store)
        var radio = PlayRequest(address: "https://radio.example/jazz.mp3", title: "Jazz FM", detail: "Music", kind: "radio",
                                titleID: "", videoID: "", poster: nil, source: "")
        radio.remember = false
        player.play(radio)
        #expect(player.isRadio)
        player.leaving()
        #expect(!player.paused)
        player.close()
        player.play(PlayRequest(address: "https://x/f.mp4", title: "A Film", detail: "", kind: "movie", titleID: "tt1",
                                videoID: "tt1", poster: nil, source: ""))
        player.leaving()
        #expect(player.paused)
        player.close()
    }
}
