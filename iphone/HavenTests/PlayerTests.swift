import Foundation
import Testing
@testable import Haven

/// The Rust build's player tests (`haven-player/src/tracks.rs`,
/// `haven-app/src/player.rs`, `external.rs`), ported, and the player's
/// remembering and moving on.
struct PlayerRulesTests {
    // Made up in the shape mpv gives for `track-list`.
    private static let sample = """
        [
            {"id": 1, "type": "video", "selected": true, "codec": "h264", "demux-w": 1920},
            {"id": 1, "type": "audio", "lang": "eng", "title": "English", "selected": true,
             "codec": "ac3", "demux-channel-count": 6},
            {"id": 2, "type": "audio", "lang": "eng", "title": "Commentary", "selected": false,
             "codec": "aac", "demux-channel-count": 2},
            {"id": 3, "type": "audio", "selected": false},
            {"id": 1, "type": "sub", "lang": "nl", "selected": false, "codec": "subrip"},
            {"id": 2, "type": "sub", "lang": "en", "title": "Signs", "forced": true, "selected": true},
            {"id": 3, "type": "sub", "lang": "und", "title": "  ", "selected": false},
            {"type": "sub"},
            "not a track"
        ]
        """

    @Test func namesAudioAndSubtitleTracks() {
        let seen = PlayerRules.tracks(Self.sample).map { "\($0.kind == .audio ? "a" : "s")\($0.id) \($0.label) \($0.selected)" }
        #expect(seen == [
            "a1 English · AC3 5.1 true",
            "a2 English · Commentary · AAC Stereo false",
            "a3 Track 3 false",
            "s1 Dutch false",
            "s2 English · Signs · Forced true",
            "s3 Track 3 false",
        ])
    }

    @Test func anythingThatIsNotATrackListGivesNoTracks() {
        #expect(PlayerRules.tracks("").isEmpty)
        #expect(PlayerRules.tracks("{}").isEmpty)
        #expect(PlayerRules.tracks("[]").isEmpty)
    }

    @Test func unknownLanguageCodesAreShownAsTheyAre() {
        #expect(PlayerRules.languageName("pt-BR") == "Portuguese")
        #expect(PlayerRules.languageName("ENG") == "English")
        #expect(PlayerRules.languageName("tlh") == "TLH")
    }

    @Test func aStreamsArrivalIsPutInWords() {
        #expect(PlayerRules.arrival(4_200_000, loaded: nil) == "4.2 MB/s")
        #expect(PlayerRules.arrival(850_000, loaded: 42.7) == "850 KB/s  ·  42% loaded")
        #expect(PlayerRules.arrival(nil, loaded: 99.8) == "100% loaded")
        #expect(PlayerRules.arrival(0, loaded: nil) == "")
        #expect(PlayerRules.arrival(nil, loaded: nil) == "")
        let status = MPV.Status(position: 600, duration: 6000, paused: false, buffering: false, arriving: nil, ahead: 600)
        #expect(PlayerRules.shareLoaded(status) == 20)
        #expect(PlayerRules.shareLoaded(MPV.Status(position: 5, duration: 0, paused: false, buffering: false)) == nil)
    }

    @Test func playerAppsAreHandedTheAddressSpeltOut() {
        let address = "https://example.com/film.mkv?x=1&y=2"
        #expect(PlayerRules.otherPlayerLink(kind: 1, address: address)
            == "vlc-x-callback://x-callback-url/stream?url=https%3A%2F%2Fexample.com%2Ffilm.mkv%3Fx%3D1%26y%3D2")
        #expect(PlayerRules.otherPlayerLink(kind: 2, address: address).hasPrefix("infuse://x-callback-url/play?url=https%3A%2F%2F"))
        #expect(PlayerRules.otherPlayerLink(kind: 3, address: address) == "outplayer://\(address)")
        // Files inside Haven always play in Haven's own player.
        let vlc = PlayerRules.OtherPlayer(kind: 1)
        #expect(PlayerRules.goesElsewhere(vlc, address: address))
        #expect(!PlayerRules.goesElsewhere(vlc, address: "/var/mobile/Downloads/film.mkv"))
        #expect(!PlayerRules.goesElsewhere(vlc, address: "file:///var/film.mkv"))
        #expect(!PlayerRules.goesElsewhere(PlayerRules.OtherPlayer(), address: address))
    }

    @Test func theSettingsReadAsTheRustBuildWroteThem() throws {
        let read = try JSONDecoder().decode(PlayerRules.Preferences.self,
                                            from: Data(#"{"volume": 0.4, "muted": true, "subtitle_scale": 1.3, "passthrough": true}"#.utf8))
        #expect(read.volume == 0.4 && read.muted && read.subtitleScale == 1.3 && read.passthrough)
        let empty = try JSONDecoder().decode(PlayerRules.Preferences.self, from: Data("{}".utf8))
        #expect(empty == PlayerRules.Preferences())
        let written = String(decoding: try JSONEncoder().encode(read), as: UTF8.self)
        #expect(written.contains("\"subtitle_scale\""))
        let other = try JSONDecoder().decode(PlayerRules.OtherPlayer.self, from: Data(#"{"kind": 2}"#.utf8))
        #expect(other.kind == 2 && other.program.isEmpty)
    }

    @Test func theControlsSayWhatIsKnown() {
        let utc = TimeZone(identifier: "UTC")!
        let now = Date(timeIntervalSince1970: 20 * 3600)
        #expect(PlayerRules.endsAt(left: 5400, speed: 1, now: now, zone: utc) == "Ends at 21:30")
        #expect(PlayerRules.endsAt(left: 5400, speed: 2, now: now, zone: utc) == "Ends at 20:45")
        let details = MPV.Details(width: 1920, height: 1080, framesPerSecond: 23.976, video: "h264", audio: "eac3",
                                  channels: "5.1", container: "mkv", decoder: "videotoolbox")
        #expect(PlayerRules.info(details, source: "Home (Jellyfin)") == """
            Picture: 1920 × 1080, 23.976 frames a second
            Video: h264 (decoded by videotoolbox)
            Sound: eac3, 5.1
            File: mkv
            From: Home (Jellyfin)
            """)
        #expect(PlayerRules.info(MPV.Details(), source: "") == "")
        #expect(PlayerRules.speedLabel(1) == "Normal" && PlayerRules.speedLabel(1.5) == "1.5×" && PlayerRules.speedLabel(2) == "2×")
    }

    @Test func mpvsReasonsAreInWords() {
        #expect(MPV.reason(-13) == "the address could not be opened")
        #expect(MPV.reason(-17) == "the format was not recognised")
        #expect(MPV.reason(-16) == "there was no picture or sound in it")
        #expect(MPV.reason(-99) == "mpv error -99")
    }
}

@MainActor
struct PlayerTests {
    private func request(_ video: String = "tt7700003", next: String? = nil) -> PlayRequest {
        PlayRequest(address: "https://x/f.mp4", title: "A Film", detail: "", kind: "movie", titleID: "tt7700003",
                    videoID: video, poster: nil, source: "Somewhere", next: next, serverItem: "srv:4:abc")
    }

    @Test func onlyAPlacePartwayOrAtTheEndIsKept() throws {
        let store = try Store.inMemory()
        PlayerModel.remember(request(), position: 10, duration: 6000, store: store)
        #expect(try store.progress(kind: "movie", titleID: "tt7700003", videoID: "tt7700003") == nil, "barely started")
        PlayerModel.remember(request(), position: 600, duration: 6000, store: store)
        let partway = try #require(try store.progress(kind: "movie", titleID: "tt7700003", videoID: "tt7700003"))
        #expect(partway.position == 600 && !partway.finished && partway.title == "A Film")
        PlayerModel.remember(request(), position: 5800, duration: 6000, store: store)
        #expect(try store.progress(kind: "movie", titleID: "tt7700003", videoID: "tt7700003")?.finished == true)
        var live = request()
        live.remember = false
        #expect(PlayerModel.played(live, position: 600, duration: 6000, failed: false) == nil)
        #expect(PlayerModel.played(request(), position: 600, duration: 6000, failed: true) == nil)
        #expect(PlayerModel.played(request(), position: 600, duration: 0, failed: false) == nil, "a live stream")
        let played = try #require(PlayerModel.played(request(), position: 5800, duration: 6000, failed: false))
        #expect(played.finished && played.serverItem == "srv:4:abc")
    }

    @Test func playingResumesWhereItWasLeft() throws {
        let store = try Store.inMemory()
        PlayerModel.remember(request(), position: 600, duration: 6000, store: store)
        let player = PlayerModel(store: store)
        player.play(request(next: "S1 E2 · Two"))
        #expect(player.open && player.busy && player.position == 597, "3 s back")
        #expect(player.nextTitle == "S1 E2 · Two" && player.title == "A Film")
        var closed: [Played?] = []
        player.closed = { closed.append($0) }
        player.close()
        #expect(!player.open && closed.count == 1)
        #expect(closed.first! == nil, "it never loaded: its length is unknown, so nothing is recorded")
        #expect(try store.progress(kind: "movie", titleID: "tt7700003", videoID: "tt7700003")?.position == 600, "the place stays")
    }

    @Test func theSettingsAreKept() throws {
        let store = try Store.inMemory()
        let player = PlayerModel(store: store)
        player.setVolume(0.3)
        player.toggleMute()
        player.stepSubtitleScale(1)
        player.stepSubtitleScale(1)
        let again = PlayerModel(store: store)
        #expect(again.preferences.volume == 0.3 && again.preferences.muted)
        #expect(again.preferences.subtitleScale == 1.2)
        // Moving the volume turns the sound back on.
        again.setVolume(0.5)
        #expect(!again.preferences.muted)
        for _ in 0 ..< 30 { again.stepSubtitleScale(-1) }
        #expect(again.preferences.subtitleScale == 0.5)
        again.stepSubtitleDelay(1)
        again.stepSubtitleDelay(1)
        #expect(again.subtitleDelay == 1)
        again.setSpeed(1.5)
        again.setFit(.fill)
        #expect(again.speed == 1.5 && again.fit == .fill)
    }

    @Test func afterPlayingTheServerAndAccountsAreTold() async throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("haven-\(UUID().uuidString)")
        let library = Library(store: try Store.inMemory(), keys: Secrets(), http: HTTP(protocolClasses: [FixtureServer.self]), kept: Kept(dir: folder))
        var queued: [String] = []
        library.queueSync = { what, _, _, _ in queued.append(what) }
        library.playerClosed(Played(kind: "movie", titleID: "tt1", videoID: "tt1", position: 5800, duration: 6000, finished: true))
        library.playerClosed(Played(kind: "movie", titleID: "tt1", videoID: "tt1", position: 600, duration: 6000, finished: false))
        library.playerClosed(nil)
        #expect(queued == ["watched"])
        #expect(library.marks == 3)
        // Nothing open: no next episode.
        #expect(!library.playNext(nil))
    }

    @Test func theNextEpisodeOpensAndGivesWayWhenNothingCanPlayIt() async throws {
        FixtureServer.answer(try ResourceURL.make(Cinemeta.url, resource: "meta", kind: "series", id: "tt7700004"), """
            {"meta": {"id": "tt7700004", "type": "series", "name": "B Show",
              "videos": [{"id": "tt7700004:1:1", "title": "One", "season": 1, "episode": 1},
                         {"id": "tt7700004:2:1", "title": "Two", "season": 2, "episode": 1}]}}
            """)
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("haven-\(UUID().uuidString)")
        let library = Library(store: try Store.inMemory(), keys: Secrets(), http: HTTP(protocolClasses: [FixtureServer.self]), kept: Kept(dir: folder))
        var dismissed = 0
        library.dismissPlayer = { dismissed += 1 }
        let page = library.title!
        page.open(Opening(id: "tt7700004", kind: "series", title: "B Show"))
        for _ in 0 ..< 100 where page.loading { try await Task.sleep(for: .milliseconds(30)) }
        page.openEpisode("tt7700004:1:1")
        #expect(page.nextVideo?.id == "tt7700004:2:1")
        #expect(!library.playNext(nil), "nothing played yet: no place to play from")
        library.lastPlayed = ("Home (Jellyfin)", "1080p")
        // Nobody to ask: the player gives way to the list straight away.
        #expect(!library.playNext(nil))
        #expect(dismissed == 1)
        #expect(page.season == 2 && page.episodeLabel == "S2 E1 · Two")
        #expect(page.sources.autoplay == nil)
    }
}
