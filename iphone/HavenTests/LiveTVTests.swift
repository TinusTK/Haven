import Foundation
import Testing
@testable import Haven

/// The Rust build's Live TV tests (haven-livetv lib, m3u, xtream) and the
/// player's address tests (haven-player source), ported.
struct M3UTests {
    private func channel(_ name: String, _ group: String?) -> Channel {
        Channel(name: name, address: "http://example.org/a", group: group)
    }

    @Test func groupsKeepTheirFirstOrder() {
        let channels = [channel("A", "News"), channel("B", nil), channel("C", "Sport"), channel("D", "News")]
        #expect(M3U.groups(channels) == ["News", "Sport"])
    }

    @Test func filterTakesAGroupAndWordsInAnyCase() {
        let one = channel("BBC World News HD", "News")
        #expect(M3U.matches(one, group: nil, words: ""))
        #expect(M3U.matches(one, group: "News", words: "world bbc"))
        #expect(!M3U.matches(one, group: "Sport", words: ""))
        #expect(!M3U.matches(one, group: nil, words: "cnn"))
        #expect(!M3U.matches(channel("X", nil), group: "News", words: ""))
    }

    @Test func aSourceIsNamedAfterItsHost() {
        #expect(M3U.sourceName("https://tv.example.org/lists/all.m3u") == "tv.example.org")
        #expect(M3U.sourceName("http://user:secret@10.0.0.2:8080/get.php?x=1") == "10.0.0.2")
        #expect(M3U.sourceName("http://[::1]:80/a") == "[::1]")
        #expect(M3U.sourceName("nonsense") == "nonsense")
    }

    // Made up for these tests.
    static let sample = "\u{feff}#EXTM3U url-tvg=\"http://example.org/guide.xml\"\r\n"
        + "#EXTINF:-1 tvg-id=\"one.example\" tvg-logo=\"http://example.org/one.png\" group-title=\"News\",Channel One\r\n"
        + "#EXTVLCOPT:http-user-agent=Example\r\n"
        + "http://example.org/one.m3u8\r\n"
        + "\r\n"
        + "#EXTINF:-1 group-title=\"Sport;Local\" tvg-name=\"Unused\",Two, the \"Second\"\n"
        + "http://example.org/two.ts\n"
        + "#extinf:0 radio=\"true\" Group-Title=\"Music\",Radio Three\n"
        + "#EXTGRP:Ignored because a group is set\n"
        + "http://example.org/three.mp3\n"
        + "#EXTINF:-1,Four\n"
        + "#EXTGRP:Old Style\n"
        + "rtsp://example.org/four\n"
        + "#EXTINF:-1 tvg-name=\"Named By Attribute\",\n"
        + "http://example.org/five\n"
        + "#EXTINF:-1,No address follows\n"
        + "#EXTINF:-1 tvg-logo=\"\",Six\n"
        + "http://example.org/six\n"
        + "http://example.org/path/seven.m3u8?token=1\n"

    @Test func readsChannelsWithTheirDetails() {
        let channels = M3U.parse(Self.sample)
        #expect(channels.map(\.name) == ["Channel One", "Two, the \"Second\"", "Radio Three", "Four", "Named By Attribute", "Six", "seven.m3u8"])
        let one = channels[0]
        #expect(one.address == "http://example.org/one.m3u8")
        #expect(one.group == "News")
        #expect(one.logo == "http://example.org/one.png")
        #expect(one.guideID == "one.example")
        #expect(!one.radio)
        #expect(channels[1].group == "Sport")
        #expect(channels[2].radio)
        #expect(channels[2].group == "Music")
        #expect(channels[3].group == "Old Style")
        #expect(channels[3].address == "rtsp://example.org/four")
        #expect(channels[5].logo == nil)
        #expect(channels[6].group == nil)
    }

    @Test func textThatIsNotAPlaylistGivesWhatItCan() {
        #expect(M3U.parse("").isEmpty)
        #expect(M3U.parse("#EXTM3U\n#EXTINF:-1,Alone\n").isEmpty)
        // A web page instead of a playlist: lines are taken as addresses,
        // which the app then refuses as unplayable.
        #expect(M3U.parse("<html>").count == 1)
        #expect(LiveTV.playable(M3U.parse("<html>")).isEmpty)
    }
}

struct XtreamTests {
    private var account: XtreamAccount { XtreamAccount(server: "tv.example.org:8080", username: "me", password: "p&ss word")! }

    @Test func theTypedServerIsTidied() throws {
        #expect(account.server == "http://tv.example.org:8080")
        let pasted = try #require(XtreamAccount(server: " HTTPS://tv.example.org/get.php?username=x&password=y&type=m3u_plus ",
                                                username: " me ", password: " secret "))
        #expect(pasted.server == "https://tv.example.org" && pasted.username == "me" && pasted.password == "secret")
        for (server, user, password) in [("", "me", "x"), ("tv.example.org", "", "x"), ("tv.example.org", "me", ""), ("ftp://tv.example.org", "me", "x")] {
            #expect(XtreamAccount(server: server, username: user, password: password) == nil, "\(server)")
        }
    }

    @Test func anAccountSurvivesBeingStoredAsAnAddress() {
        let url = account.sourceURL
        #expect(url == "http://tv.example.org:8080/player_api.php?username=me&password=p%26ss%20word")
        #expect(XtreamAccount(sourceURL: url) == account)
        #expect(account.api("get_live_streams") == "\(url)&action=get_live_streams")
        // A playlist address, or an API address with an action, is not an account.
        #expect(XtreamAccount(sourceURL: "http://tv.example.org/get.php?username=a&password=b") == nil)
        #expect(XtreamAccount(sourceURL: account.api("x")) == nil)
        #expect(account.label == "me at tv.example.org")
        #expect(!String(describing: account).contains("ss word") && !String(reflecting: account).contains("ss word"))
        // A "+" in a stored address is a space.
        #expect(XtreamAccount(sourceURL: "http://a.example/player_api.php?username=b+c&password=d")?.username == "b c")
    }

    // Made up in the shape Xtream Codes servers answer with.
    @Test func signingInReadsTheAccountState() throws {
        let fine = #"{"user_info": {"auth": 1, "status": "Active", "exp_date": "1900000000", "allowed_output_formats": ["m3u8", "ts"]}, "server_info": {}}"#
        #expect(try Xtream.signIn(fine) == XtreamInfo(expires: 1_900_000_000, formats: ["m3u8", "ts"]))
        #expect(throws: XtreamError.refused) { try Xtream.signIn(#"{"user_info": {"auth": 0}}"#) }
        #expect(throws: XtreamError.notActive("expired")) { try Xtream.signIn(#"{"user_info": {"auth": "1", "status": "Expired"}}"#) }
        #expect(throws: XtreamError.badAnswer) { try Xtream.signIn("<html>") }
        #expect(throws: XtreamError.badAnswer) { try Xtream.signIn("[]") }
        #expect(try Xtream.signIn(#"{"user_info": {"auth": 1, "exp_date": null}}"#) == XtreamInfo())
    }

    @Test func providerNamesAreStrippedDownToTheTitle() {
        for (name, plain, year) in [
            ("EN - Dune: Part Two (2024)", "duneparttwo", "2024"),
            ("Dune Part Two 2024 4K", "duneparttwo", "2024"),
            ("UK| The Film [Multi-Sub] HD", "thefilm", nil),
            ("2001: A Space Odyssey (1968)", "2001aspaceodyssey", "1968"),
            ("1917", "1917", nil),
            ("Blade Runner 2049", "bladerunner", "2049"),
        ] as [(String, String, String?)] {
            let got = Xtream.plainTitle(name)
            #expect(got.plain == plain && got.year == year, "\(name)")
        }
    }

    @Test func filmsAndSeriesAreMatchedAndEpisodesFound() throws {
        let films = try Xtream.films("""
            [{"name": "EN - A Film (2021)", "stream_id": 11, "container_extension": "mkv"},
             {"name": "A Film 2021 4K", "stream_id": "12", "container_extension": "mp4", "tmdb": "500"},
             {"name": "A Film (1984)", "stream_id": 13, "container_extension": "avi"},
             {"name": "Another Film", "stream_id": 14, "tmdb": "0", "year": "2021"},
             {"name": "No id"}]
            """)
        #expect(films.count == 4)
        // The one with a TMDB number is judged by it alone.
        #expect(Xtream.matches(films, name: "A Film", year: "2021", tmdb: "500").map(\.id) == ["11", "12"])
        #expect(Xtream.matches(films, name: "A Film", year: "2021", tmdb: "999").map(\.id) == ["11"])
        #expect(Xtream.matches(films, name: "A Film", year: "1984", tmdb: nil).map(\.id) == ["13"])
        #expect(Xtream.matches(films, name: "", year: nil, tmdb: nil).isEmpty)
        #expect(throws: XtreamError.badAnswer) { try Xtream.films("{}") }

        let series = try Xtream.series(#"[{"name": "A Show", "series_id": 7, "releaseDate": "2019-04-01"}]"#)
        #expect(series[0].id == "7" && series[0].year == "2019")

        let info = #"{"episodes": {"1": [{"id": "101", "episode_num": 1, "season": 1, "container_extension": "mkv"}, {"id": "102", "episode_num": "2", "season": "1", "container_extension": "mp4"}], "2": [{"id": "201", "episode_num": 1, "season": 2}]}}"#
        #expect(Xtream.episode(info, season: 1, episode: 2)! == ("102", "mp4"))
        #expect(Xtream.episode(info, season: 2, episode: 1)! == ("201", ""))
        #expect(Xtream.episode(info, season: 3, episode: 1) == nil)
        let listOfLists = #"{"episodes": [[{"id": "9", "episode_num": 4, "season": 3}]]}"#
        #expect(Xtream.episode(listOfLists, season: 3, episode: 4)?.id == "9")

        #expect(account.film(id: "12", extension: "mp4") == "http://tv.example.org:8080/movie/me/p%26ss%20word/12.mp4")
        #expect(account.episode(id: "201", extension: "") == "http://tv.example.org:8080/series/me/p%26ss%20word/201.mp4")
    }

    @Test func channelsComeWithGroupsAndSurviveThePlaylistForm() throws {
        let categories = #"[{"category_id": "1", "category_name": "News; World"}, {"category_id": "2", "category_name": "Music"}]"#
        let streams = """
            [{"num": 1, "name": "Channel \\"One\\"", "stream_type": "live", "stream_id": 101,
              "stream_icon": "http://tv.example.org/one.png", "epg_channel_id": "one.example", "category_id": "1"},
             {"name": "Radio Two", "stream_type": "radio_streams", "stream_id": "202", "category_id": 2},
             {"name": "No category", "stream_id": 303, "category_id": "9", "stream_icon": ""},
             {"name": "No id"}, {"stream_id": 404}]
            """
        let info = XtreamInfo()
        let channels = try Xtream.channels(account, info: info, categories: categories, streams: streams)
        #expect(channels.count == 3)
        #expect(channels[0].address == "http://tv.example.org:8080/live/me/p%26ss%20word/101.ts")
        #expect(channels[0].group == "News; World")
        #expect(channels[0].guideID == "one.example")
        #expect(channels[1].radio && !channels[0].radio)
        #expect(channels[1].group == "Music")
        #expect(channels[2].group == nil && channels[2].logo == nil)

        let readBack = M3U.parse(M3U.write(channels))
        #expect(readBack.count == 3)
        #expect(readBack[0].name == "Channel \"One\"")
        #expect(readBack[0].address == channels[0].address)
        #expect(readBack[0].group == "News, World")
        #expect(readBack[0].logo == channels[0].logo)
        #expect(readBack[1].radio)
        #expect(readBack[2] == channels[2])

        // No category list still gives the channels.
        #expect(try Xtream.channels(account, info: info, categories: "oops", streams: streams).count == 3)
        #expect(throws: XtreamError.badAnswer) { try Xtream.channels(account, info: info, categories: categories, streams: "{}") }
        let hls = XtreamInfo(formats: ["m3u8"])
        #expect(try Xtream.channels(account, info: hls, categories: categories, streams: streams)[0].address.hasSuffix("/101.m3u8"))
    }
}

struct PlayableAddressTests {
    @Test func webAddressesPass() {
        #expect(PlayableAddress.web("https://example.org/a.mp4") == "https://example.org/a.mp4")
        #expect(PlayableAddress.web("  HTTP://10.0.0.2:8080/live.m3u8\n") == "HTTP://10.0.0.2:8080/live.m3u8")
        #expect(PlayableAddress.web("http://[::1]/v.mkv") == "http://[::1]/v.mkv")
    }

    @Test func televisionAddressesPassAsStreamsOnly() {
        for address in ["rtsp://10.0.0.5/live", "udp://@239.0.0.1:1234", "RTMP://example.org/app/key"] {
            #expect(PlayableAddress.stream(address) == address)
            #expect(PlayableAddress.web(address) == nil)
        }
        #expect(PlayableAddress.stream("https://example.org/a.m3u8") == "https://example.org/a.m3u8")
        for address in ["file:///etc/passwd", "/dev/video0", "edl://x", "<html>", "https://@"] {
            #expect(PlayableAddress.stream(address) == nil, "\(address)")
        }
    }

    @Test func everythingElseIsRefused() {
        for address in ["", "/etc/passwd", "file:///etc/passwd", "edl://a.mkv", "lavfi://testsrc", "av://v4l2:/dev/video0",
                        "smb://server/share/a.mkv", "magnet:?xt=urn:btih:abc", "https://", "https:///no-host",
                        "https://example.org/a b.mp4", "https://example.org/a\nb.mp4", "https://@example.org/a.mp4",
                        "javascript://https://example.org"] {
            #expect(PlayableAddress.web(address) == nil, "\(address)")
        }
    }
}

/// What the Live TV screens show, and adding sources.
struct LiveTVTests {
    private let channels = [
        Channel(name: "BBC One", address: "http://a/1", group: "News"),
        Channel(name: "Sky Sports", address: "http://a/2", group: "Sport"),
        Channel(name: "BBC News", address: "http://a/3", group: "News"),
    ]

    @Test func categoriesListFavouritesThenAllThenGroups() {
        let favourites = [LiveTV.FavouriteGroup(name: "Mine", channels: ["http://a/2"]),
                          LiveTV.FavouriteGroup(name: "Empty")]
        let rows = LiveTV.categories(channels, radio: false, favourites: favourites, words: "")
        #expect(rows.map(\.key) == ["f:Mine", "f:Empty", "", "g:News", "g:Sport"])
        #expect(rows.map(\.count) == [1, 0, 3, 2, 1])
        #expect(rows[2].label == "All channels")
        // Radio lists only groups that hold stations; the filter leaves favourites alone.
        let radio = LiveTV.categories(channels, radio: true, favourites: favourites, words: "spo")
        #expect(radio.map(\.key) == ["f:Mine", "", "g:Sport"])
        #expect(radio[1].label == "All stations")
        #expect(LiveTV.categoryExists("g:News", channels: channels, favourites: favourites))
        #expect(!LiveTV.categoryExists("g:Film", channels: channels, favourites: favourites))
        #expect(LiveTV.categoryExists("f:Empty", channels: channels, favourites: favourites))
    }

    @Test func channelsAreFilteredAndStarred() {
        let favourites = [LiveTV.FavouriteGroup(name: "Mine", channels: ["http://a/3"])]
        let news = LiveTV.channelRows(channels, category: "g:News", words: "bbc", favourites: favourites)
        #expect(news.map(\.index) == [0, 2])
        #expect(news.map(\.starred) == [false, true])
        #expect(LiveTV.channelRows(channels, category: "f:Mine", words: "", favourites: favourites).map(\.name) == ["BBC News"])
        #expect(LiveTV.channelRows(channels, category: "", words: "one bbc", favourites: favourites).map(\.name) == ["BBC One"])
    }

    @Test func favouriteGroupsAreMadeFilledAndDeleted() throws {
        var favourites: [LiveTV.FavouriteGroup] = []
        LiveTV.newGroup(" Sport ", adding: "http://a/2", to: &favourites)
        LiveTV.newGroup("Sport", adding: "http://a/2", to: &favourites)
        LiveTV.newGroup("  ", adding: nil, to: &favourites)
        #expect(favourites == [LiveTV.FavouriteGroup(name: "Sport", channels: ["http://a/2"])])
        LiveTV.toggle("http://a/1", group: "Sport", in: &favourites)
        LiveTV.toggle("http://a/2", group: "Sport", in: &favourites)
        #expect(favourites[0].channels == ["http://a/1"])
        let store = try Store.inMemory()
        LiveTV.save(favourites, store: store)
        #expect(LiveTV.favourites(store) == favourites)
        #expect(try store.get(LiveTV.favouritesKey, as: [LiveTV.FavouriteGroup].self)?.count == 1)
        LiveTV.deleteGroup("Sport", from: &favourites)
        #expect(favourites.isEmpty)
    }

    @Test func addingAPlaylistKeepsItAndSaysWhatCame() throws {
        let store = try Store.inMemory()
        let url = "http://lists.example/all.m3u"
        let fetched = LiveTV.fetched(url, account: nil, adding: true, .success(M3UTests.sample), store: store)
        #expect(fetched.saved)
        #expect(fetched.status == .init(text: "Added 6 channels and 1 radio station. They are on the Live TV tab.", isError: false, account: false))
        let boards = LiveTV.boards(store)
        #expect(boards.tv.count == 6 && boards.radio.count == 1)
        #expect(LiveTV.sourceRows(store).map(\.name) == ["lists.example"])

        // An empty list keeps what was there; a failed refresh says nothing.
        let empty = LiveTV.fetched(url, account: nil, adding: false, .success("#EXTM3U"), store: store)
        #expect(!empty.saved && empty.status == nil)
        #expect(LiveTV.boards(store).tv.count == 6)
        #expect(LiveTV.fetched(url, account: nil, adding: true, .success("<html>"), store: store).status?.text
            == "No channels were found at that address.")
        #expect(LiveTV.fetched(url, account: nil, adding: true, .failure(Problem("the server answered with error 404")), store: store).status?.text
            == "Could not add the source: the server answered with error 404.")

        // Switched off: listed, its channels not shown. Removed: gone, and
        // a refresh on the way does not bring it back.
        #expect(LiveTV.setEnabled(url, on: false, store: store) == nil)
        #expect(LiveTV.boards(store).tv.isEmpty && LiveTV.sourceRows(store)[0].enabled == false)
        #expect(LiveTV.remove(url, store: store).text == "Source removed.")
        #expect(try SwitchedOff.load(store).tvSources.isEmpty)
        #expect(!LiveTV.fetched(url, account: nil, adding: false, .success(M3UTests.sample), store: store).saved)
        #expect(LiveTV.sourceRows(store).isEmpty)
    }

    @Test func anAccountIsListedWithoutItsPassword() throws {
        let store = try Store.inMemory()
        let account = XtreamAccount(server: "tv.example.org", username: "me", password: "secret")!
        let playlist = M3U.write([Channel(name: "One", address: account.stream(id: "1", format: "ts"))])
        let fetched = LiveTV.fetched(account.sourceURL, account: account, adding: true, .success(playlist), store: store)
        #expect(fetched.online == true && fetched.status?.text == "Added 1 channel. They are on the Live TV tab.")
        #expect(fetched.status?.account == true)
        let row = try #require(LiveTV.sourceRows(store).first)
        #expect(row.name == "me at tv.example.org" && row.detail == "Xtream Codes account of me" && row.isAccount)
        #expect(LiveTV.accounts(store) == [account])
        #expect(LiveTV.fetched(account.sourceURL, account: account, adding: false, .failure(Problem("x")), store: store).online == false)
        #expect(LiveTV.playlistAddress("not an address") == .failure(.init(text: "Could not add the source: \"not an address\" is not a web address.", isError: true, account: false)))
    }

    @Test func anAccountIsFetchedThroughItsAPI() async throws {
        let account = XtreamAccount(server: "xt1.test", username: "u", password: "p")!
        FixtureServer.answer(account.api(), #"{"user_info": {"auth": 1, "status": "Active"}}"#)
        FixtureServer.answer(account.api("get_live_streams"), #"[{"name": "One", "stream_id": 5, "category_id": "1"}]"#)
        let http = HTTP(protocolClasses: [FixtureServer.self])
        let text = try await LiveTV.fetch(account.sourceURL, http: http).get()
        #expect(M3U.parse(text).map(\.address) == ["http://xt1.test/live/u/p/5.ts"])
        #expect(await LiveTV.check(account, http: http))
        let refused = XtreamAccount(server: "xt2.test", username: "u", password: "bad")!
        FixtureServer.answer(refused.api(), #"{"user_info": {"auth": 0}}"#)
        #expect(await LiveTV.fetch(refused.sourceURL, http: http) == .failure(Problem("the user name or password was not accepted")))
        #expect(await !LiveTV.check(refused, http: http))
    }

    @Test func filmsAndEpisodesComeFromAnAccountsLists() async throws {
        let account = XtreamAccount(server: "xt3.test", username: "u", password: "p")!
        FixtureServer.answer(account.api("get_vod_streams"), #"[{"name": "EN - A Film (2021)", "stream_id": 11, "container_extension": "mkv"}]"#)
        FixtureServer.answer(account.api("get_series"), #"[{"name": "A Show", "series_id": 7}]"#)
        FixtureServer.answer("\(account.api("get_series_info"))&series_id=7", #"{"episodes": {"1": [{"id": "101", "episode_num": 2, "season": 1}]}}"#)
        let lists = XtreamOnDemand(http: HTTP(protocolClasses: [FixtureServer.self]))
        let films = try await lists.streams(account, wanted: Wanted(name: "A Film", year: "2021")).get()
        #expect(films.map(\.url) == ["http://xt3.test/movie/u/p/11.mkv"])
        #expect(films[0].description == "MKV" && films[0].name == "EN - A Film (2021)")
        let episode = try await lists.streams(account, wanted: Wanted(series: true, name: "A Show", episode: (1, 2))).get()
        #expect(episode.map(\.name) == ["A Show  S1 E2"])
        #expect(episode[0].url == "http://xt3.test/series/u/p/101.mp4" && episode[0].description == "File")
        #expect(!XtreamOnDemand.worthAsking(Wanted(series: true, name: "A Show")))
        #expect(XtreamOnDemand.sourceLabel(account) == "Xtream · u at xt3.test")
        let broken = XtreamAccount(server: "xt4.test", username: "u", password: "p")!
        #expect(await lists.streams(broken, wanted: Wanted(name: "A Film")) == .failure(Problem("the server answered with error 404")))
    }
}
