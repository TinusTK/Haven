import Foundation
import Testing
@testable import Haven

/// JSON text as a server's answer.
private func json(_ text: String) -> Any {
    try! JSONSerialization.jsonObject(with: Data(text.utf8), options: [.fragmentsAllowed])
}

private func hex(_ text: String) -> String { Servers.hex(text) }

/// A client that asks the stand-in server and leaves the network rule out.
private func stubClient() -> ServerClient {
    var client = ServerClient(device: "dev-1", http: HTTP(protocolClasses: [FixtureServer.self]))
    client.ownNetworkOnly = false
    let config = URLSessionConfiguration.ephemeral
    config.protocolClasses = [FixtureServer.self]
    client.streaming = URLSession(configuration: config)
    return client
}

/// The Rust build's shared server tests (haven-servers lib.rs), ported.
struct ServerRulesTests {
    @Test func idsRoundTripAndOtherIDsAreLeftAlone() {
        #expect(Servers.titleID(server: 3, item: "abc") == "srv:3:abc")
        #expect(Servers.splitID("srv:3:abc")! == (3, "abc"))
        #expect(Servers.splitID("srv:12:a:b")! == (12, "a:b"))
        #expect(Servers.splitID("tt0903747") == nil)
        #expect(Servers.splitID("srv:x:abc") == nil)
        #expect(Servers.splitID("srv:3:") == nil)
    }

    @Test func addressesAreTidied() throws {
        #expect(try Servers.address(" 192.168.1.5:8096/ ") == "http://192.168.1.5:8096")
        #expect(try Servers.address("https://media.example.org/jf/") == "https://media.example.org/jf")
        #expect(throws: ServerError.self) { try Servers.address("ftp://x") }
        #expect(throws: ServerError.self) { try Servers.address("") }
    }

    @Test func theWantedTitleIsMatchedByIDThenByNameAndYear() {
        let onServer = [
            Candidate(id: "a", name: "The Film", year: "1990", imdb: "tt1"),
            Candidate(id: "b", name: "The Film", year: "2021", imdb: "tt2"),
            Candidate(id: "c", name: "Another: Film!", year: "2005", imdb: nil),
        ]
        func wanted(_ name: String, _ year: String, _ imdb: String?) -> Wanted {
            Wanted(name: name, year: year, imdb: imdb)
        }
        // The id decides, whatever the name says.
        #expect(Servers.bestMatch(wanted("Renamed", "2021", "TT2"), onServer)?.id == "b")
        // A different id of the same kind is another title, even by the same name.
        #expect(Servers.bestMatch(wanted("The Film", "1990", "tt9"), onServer) == nil)
        // No id to compare: name and year.
        #expect(Servers.bestMatch(wanted("another film", "2005-2009", nil), onServer)?.id == "c")
        #expect(Servers.bestMatch(wanted("another film", "2006", nil), onServer) == nil)
        #expect(Servers.bestMatch(wanted("", "2005", nil), onServer) == nil)
    }

    @Test func sizesAndQualitiesReadWell() {
        #expect(Servers.quality(width: 3840, height: 1600) == "4K")
        #expect(Servers.quality(width: 1920, height: 800) == "1080p")
        #expect(Servers.quality(width: nil, height: 720) == "720p")
        #expect(Servers.quality(width: 720, height: 576) == "576p")
        #expect(Servers.quality(width: 640, height: nil) == "SD")
        #expect(Servers.quality(width: nil, height: nil) == nil)
        #expect(Servers.size(4.5 * 1024 * 1024 * 1024) == "4.5 GB")
        #expect(Servers.size(700 * 1024 * 1024) == "700 MB")
        #expect(Servers.size(10) == nil)
    }

    @Test func queryTextIsEscaped() {
        #expect(Servers.query("the matrix & more") == "the%20matrix%20%26%20more")
    }

    @Test func aServerNeverPrintsItsToken() {
        let server = Server(id: 1, kind: .plex, name: "Home", address: "http://x", token: "topsecret")
        #expect(!String(describing: server).contains("topsecret"))
        #expect(!String(reflecting: server).contains("topsecret"))
    }

    @Test func storedServersComeBackAsServersAndUnknownKindsAreSkipped() throws {
        let store = try Store.inMemory()
        let a = try store.saveServer(StoredServer(kind: "jellyfin", name: "A", address: "http://a", userID: "u", token: "t"))
        try store.saveServer(StoredServer(kind: "jukebox", name: "Odd", address: "http://o", userID: "", token: "t"))
        let b = try store.saveServer(StoredServer(kind: "plex", name: "B", address: "http://b", userID: "", token: "t"))
        #expect(ServerAccounts.load(store).all.map(\.name) == ["A", "B"])
        #expect(ServerAccounts.setEnabled(a, on: false, store: store) == .init(text: "Server switched off.", isError: false))
        #expect(ServerAccounts.load(store).enabled.map(\.id) == [b])
        #expect(ServerAccounts.serverItem("srv:\(b):42", in: ServerAccounts.load(store).enabled)?.item == "42")
        #expect(ServerAccounts.serverItem("srv:\(a):42", in: ServerAccounts.load(store).enabled) == nil)
        #expect(ServerAccounts.remove(b, store: store).text == "Server removed. Haven has forgotten its sign-in.")
        #expect(ServerAccounts.load(store).all.map(\.name) == ["A"])
    }

    @Test func libraryShelvesAreNewAndByName() {
        let server = Server(id: 2, kind: .jellyfin, name: "Home", address: "http://h", token: "t")
        let plans = ServerAccounts.libraryPlans([(server, [
            MediaLibrary(id: "1", name: "Films", collection: false),
            MediaLibrary(id: "c", name: "Popular films", collection: true),
        ])])
        #expect(plans.map(\.title) == ["New in Films", "Films", "Popular films"])
        #expect(plans.map(\.latest) == [true, false, false])
        #expect(plans.allSatisfy { $0.source == "Home" })
        #expect(ServerAccounts.librariesName(server) == "libraries|2|http://h")
    }

    @Test func theDeviceIDIsMadeOnceAndKept() throws {
        let store = try Store.inMemory()
        let id = DeviceID.get(store)
        #expect(id.hasPrefix("haven-") && id.count == 38)
        #expect(DeviceID.get(store) == id)
        try store.put(DeviceID.key, "haven-fromrust")
        #expect(DeviceID.get(store) == "haven-fromrust")
    }
}

/// The Rust build's Jellyfin and Emby tests (haven-servers emby.rs), ported.
struct EmbyTests {
    private let server = Server(id: 2, kind: .jellyfin, name: "Home", address: "http://media.example", userID: "u1", token: "tok en")

    @Test func librariesKeepFilmsAndSeriesOnly() {
        let views = json("""
            {"Items": [{"Id": "a", "Name": "Films", "CollectionType": "movies"},
                       {"Id": "b", "Name": "Shows", "CollectionType": "tvshows"},
                       {"Id": "c", "Name": "Music", "CollectionType": "music"},
                       {"Id": "d", "Name": "Mixed"},
                       {"Name": "No id", "CollectionType": "movies"}]}
            """)
        #expect(Emby.readLibraries(views).map(\.name) == ["Films", "Shows", "Mixed"])
    }

    @Test func collectionsBecomeLibraries() {
        let views = json(#"{"Items": [{"Id": "a", "Name": "Movies", "CollectionType": "movies"}, {"Id": "636f6c", "Name": "Collections", "CollectionType": "boxsets"}]}"#)
        #expect(Emby.collectionViews(views) == ["636f6c"])
        #expect(Emby.readLibraries(views).count == 1)
        let found = json(#"{"Items": [{"Id": "c1", "Name": "Popular films", "Type": "BoxSet"}, {"Id": "m1", "Name": "A Film", "Type": "Movie"}, {"Name": "No id", "Type": "BoxSet"}]}"#)
        #expect(Emby.readCollections(found) == [MediaLibrary(id: "c1", name: "Popular films", collection: true)])
    }

    #if ADDONS
    @Test func sharedAddonsAreRead() {
        let config = json("""
            {"addons": [{"name": "One", "manifestUrl": "https://one.example/key123/manifest.json"},
                        {"name": "Two", "nameOverride": "Second", "manifestUrl": "https://two.example/manifest.json"},
                        {"name": "Off", "manifestUrl": "https://off.example/manifest.json", "disabled": true},
                        {"name": "No address"}, {"name": "Odd", "manifestUrl": "file:///x"}]}
            """)
        let addons = Emby.readSharedAddons(config)
        #expect(addons.map(\.name) == ["One", "Second"])
        #expect(!String(describing: addons[0]).contains("key123"))
        #expect(Emby.readSharedAddons(json("{}")).isEmpty)
    }
    #endif

    @Test func sourcesArePickedOutAsTheAnswerArrives() throws {
        let answer = #"{"Note":"MediaSources","Other":[{"Id":"no"}],"MediaSources":[{"Id":"a","Name":"One \"quoted\" } ]","MediaStreams":[{"Type":"Subtitle"}]},{"Id":"b","Nested":{"MediaSources":[{"Id":"inner"}]}}],"PlaySessionId":"x"}"#
        func ids(_ entries: [Any]) -> [String] { entries.compactMap { ServerJSON.text($0, "Id") } }
        // Whole, and a few bytes at a time: the same two entries.
        var whole = SourceScanner()
        #expect(ids(whole.feed(Array(answer.utf8))) == ["a", "b"])
        #expect(whole.done)
        var slow = SourceScanner()
        var entries: [Any] = []
        let bytes = Array(answer.utf8)
        for start in stride(from: 0, to: bytes.count, by: 7) {
            entries += slow.feed(bytes[start ..< min(start + 7, bytes.count)])
        }
        #expect(ids(entries) == ["a", "b"])
        #expect(ServerJSON.text(entries[0], "Name") == "One \"quoted\" } ]")
        // The first is there before the answer is complete.
        var early = SourceScanner()
        let cut = try #require(answer.range(of: #""Id":"b""#)).lowerBound
        #expect(ids(early.feed(Array(answer[..<cut].utf8))) == ["a"])
        #expect(!early.done)
    }

    @Test func aGatheredTitleCanBeNamedByItsIMDbID() {
        let byTMDB = hex("m|tmdb:603|movie")
        #expect(Servers.wantsIMDb(byTMDB))
        #expect(Servers.byIMDb(byTMDB, imdb: "tt0133093") == hex("m|tt0133093|movie"))
        #expect(Servers.byIMDb(hex("s|kitsu:12|series"), imdb: "tt0944947") == hex("s|tt0944947|series"))
        // Already by its IMDb id, or not a gathered title at all.
        #expect(!Servers.wantsIMDb(hex("m|tt0133093|movie")))
        #expect(Servers.byIMDb(hex("m|tt0133093|movie"), imdb: "tt0133093") == nil)
        #expect(!Servers.wantsIMDb("f27caa37e5142225cceded48f6553502"))
        #expect(Servers.byIMDb("f27caa37e5142225cceded48f6553502", imdb: "tt0133093") == nil)
        #expect(Servers.byIMDb(byTMDB, imdb: "603") == nil)
    }

    @Test func anErrorPassedOnIsNotAFilm() {
        let error = hex("m|aiostreamserror.%7B%22errorDescription%22%3A%22Rate%20Limit%22%7D|movie")
        #expect(Emby.isErrorEntry(error))
        #expect(!Emby.isErrorEntry(hex("m|tt0111161|movie")))
        #expect(!Emby.isErrorEntry("f27caa37e5142225cceded48f6553502"))
        let items = json("""
            [{"Type": "Movie", "Id": "\(error)", "Name": "Rate Limit Exceeded"},
             {"Type": "Movie", "Id": "\(hex("m|tt0111161|movie"))", "Name": "A Film"}]
            """) as! [Any]
        #expect(Emby.readPreviews(server, items).count == 1)
    }

    @Test func previewsFoldEpisodesIntoTheirSeries() {
        let items = json("""
            [{"Type": "Movie", "Id": "m1", "Name": "A Film", "ProductionYear": 2020},
             {"Type": "Episode", "Id": "e1", "Name": "Pilot", "SeriesId": "s1", "SeriesName": "A Show"},
             {"Type": "Episode", "Id": "e2", "Name": "Two", "SeriesId": "s1", "SeriesName": "A Show"},
             {"Type": "Series", "Id": "s2", "Name": "Another Show"},
             {"Type": "Audio", "Id": "x", "Name": "A Song"}]
            """) as! [Any]
        let previews = Emby.readPreviews(server, items)
        #expect(previews.map { "\($0.id) \($0.kind) \($0.name)" }
            == ["srv:2:m1 movie A Film", "srv:2:s1 series A Show", "srv:2:s2 series Another Show"])
        #expect(previews[0].releaseInfo == "2020")
        #expect(previews[0].poster == "http://media.example/Items/m1/Images/Primary?maxHeight=450&quality=90&api_key=tok%20en")
    }

    @Test func detailOfASeriesListsItsEpisodes() throws {
        let detail = json("""
            {"Id": "s1", "Type": "Series", "Name": "A Show", "Overview": "About it.",
             "ProductionYear": 2019, "CommunityRating": 8.25, "Genres": ["Drama", "Crime"],
             "RunTimeTicks": 27000000000, "BackdropImageTags": ["x"],
             "People": [{"Name": "An Actor", "Type": "Actor"}, {"Name": "A Director", "Type": "Director"}],
             "ProviderIds": {"Imdb": "tt123", "Tmdb": "456"}}
            """)
        let episodes = json(#"{"Items": [{"Id": "e1", "Name": "Pilot", "ParentIndexNumber": 1, "IndexNumber": 1, "PremiereDate": "2019-01-01T00:00:00Z"}, {"Name": "No id"}]}"#)
        let meta = try #require(Emby.readDetail(server, detail, episodes: episodes))
        #expect(meta.id == "srv:2:s1" && meta.kind == "series")
        #expect(meta.runtime == "45 min")
        #expect(meta.rating == "8.2")
        #expect(meta.genres == ["Drama", "Crime"])
        #expect(meta.cast == ["An Actor"] && meta.director == ["A Director"])
        #expect(meta.background != nil)
        #expect(meta.videos.count == 1 && meta.videos[0].id == "srv:2:e1")
        #expect(meta.imdbID == "tt123" && meta.tmdbID == "456")
        #expect(meta.videos[0].season == 1 && meta.videos[0].episode == 1)
    }

    @Test func theStreamAddressCarriesTheEscapedToken() {
        let streams = Emby.readStreams(server, item: "m1", [String: Any]())
        #expect(streams.count == 1)
        #expect(streams[0].url == "http://media.example/Videos/m1/stream?static=true&api_key=tok%20en")
    }

    @Test func everyVersionOfAFileIsAStream() {
        let detail = json("""
            {"MediaSources": [
              {"Id": "v1", "Name": "4K HDR", "Container": "mkv", "Size": 21474836480, "MediaStreams": [
                {"Type": "Video", "Codec": "hevc", "Width": 3840, "Height": 2160}, {"Type": "Audio", "Codec": "eac3"}]},
              {"Id": "v2", "Name": "HD", "Container": "mp4", "MediaStreams": [{"Type": "Video", "Codec": "h264", "Width": 1920, "Height": 1080}]},
              {"Name": "No id"}]}
            """)
        let streams = Emby.readStreams(server, item: "m1", detail)
        #expect(streams.count == 2)
        #expect(streams[0].name == "4K")
        #expect(streams[0].description == "4K HDR · MKV · HEVC · EAC3 · 20.0 GB")
        #expect(streams[0].url == "http://media.example/Videos/m1/stream?static=true&MediaSourceId=v1&api_key=tok%20en")
        #expect(streams[1].name == "1080p")
        #expect(streams[1].description == "HD · MP4 · H264")
    }

    @Test func gatheredStreamsPlayFromTheirOwnAddress() {
        let info = json("""
            {"MediaSources": [
              {"Id": "a", "Name": "Provider One 1080p WEB", "Protocol": "Http", "IsRemote": true,
               "Path": "https://streams.example/one.mkv", "Container": "mkv", "Remux": {"ProviderInfo": {"source": "Provider One"}}},
              {"Name": "Provider Two 4K", "Protocol": "Http", "Path": "https://streams.example/two.mp4",
               "MediaStreams": [{"Type": "Video", "Codec": "hevc", "Width": 3840, "Height": 2160}]},
              {"Id": "c", "Name": "A file", "Protocol": "File", "Path": "/media/file.mkv"},
              {"Name": "Nothing to play", "Protocol": "File", "Path": "/media/x.mkv"}]}
            """)
        let streams = Emby.readStreams(server, item: "m1", info)
        #expect(streams.count == 3)
        #expect(streams[0].url == "https://streams.example/one.mkv")
        #expect(streams[0].name == "Provider One 1080p WEB")
        #expect(streams[0].description == "MKV")
        #expect(streams[1].name == "4K")
        #expect(streams[1].url == "https://streams.example/two.mp4")
        #expect(streams[0].provider == "Provider One")
        #expect(streams[1].provider == nil)
        #expect(streams[2].url == "http://media.example/Videos/m1/stream?static=true&MediaSourceId=c&api_key=tok%20en")
    }

    @Test func aSourceCanNameItsOwnAddressAndTheLogNamesNoSecrets() {
        let info = json("""
            {"MediaSources": [{"Id": "a", "Name": "Secret name", "Protocol": "File", "Path": "/media/secret.mkv",
                               "DirectStreamUrl": "/Videos/m1/stream.mkv?MediaSourceId=a"}], "ErrorCode": null}
            """)
        let streams = Emby.readStreams(server, item: "m1", info)
        #expect(streams[0].url == "http://media.example/Videos/m1/stream.mkv?MediaSourceId=a&api_key=tok%20en")
        let shape = Emby.describeSources(info)
        #expect(shape.hasPrefix("lists 1 [protocol File id yes name yes path yes url yes streams 0]"), "\(shape)")
        #expect(!shape.contains("secret") && !shape.contains("Secret"))
        #expect(Emby.describeSources(json("{}")).hasPrefix("lists 0"))
    }

    @Test func picturesCarryTheirTagAndRemoteOnesAreUsedAsGiven() throws {
        let detail = json(#"{"Id": "m1", "Type": "Movie", "Name": "A Film", "ImageTags": {"Primary": "p1"}, "BackdropImageTags": ["b1"]}"#)
        let meta = try #require(Emby.readDetail(server, detail, episodes: NSNull()))
        #expect(meta.poster == "http://media.example/Items/m1/Images/Primary?maxHeight=450&quality=90&tag=p1&api_key=tok%20en")
        #expect(meta.background == "http://media.example/Items/m1/Images/Backdrop?maxHeight=900&quality=90&tag=b1&api_key=tok%20en")
        let gathered = json(#"{"Id": "m2", "Type": "Movie", "Name": "Gathered", "RemoteImages": [{"Type": "Primary", "Url": "https://pictures.example/p.jpg"}, {"Type": "Backdrop", "Url": "file:///x"}]}"#)
        let other = try #require(Emby.readDetail(server, gathered, episodes: NSNull()))
        #expect(other.poster == "https://pictures.example/p.jpg")
        #expect(other.background?.contains("/Items/m2/Images/Backdrop") == true)
    }

    @Test func candidatesCarryTheirIDsAndEpisodesAreFoundByNumber() {
        let items = json(#"{"Items": [{"Id": "m1", "Name": "A Film", "ProductionYear": 2020, "ProviderIds": {"Imdb": "tt7", "Tmdb": "42"}}, {"Id": "m2", "Name": "Bare", "ProviderIds": {"imdb": " "}}, {"Name": "No id"}]}"#)
        let candidates = Emby.readCandidates(items)
        #expect(candidates.count == 2)
        #expect(candidates[0].imdb == "tt7" && candidates[0].tmdb == "42" && candidates[0].year == "2020")
        #expect(candidates[1].imdb == nil)
        let episodes = json(#"{"Items": [{"Id": "e1", "ParentIndexNumber": 1, "IndexNumber": 1}, {"Id": "e2", "ParentIndexNumber": 1, "IndexNumber": 2}, {"Id": "e3", "ParentIndexNumber": 2, "IndexNumber": 1}]}"#)
        #expect(Emby.readEpisode(episodes, season: 2, number: 1) == "e3")
        #expect(Emby.readEpisode(episodes, season: 3, number: 1) == nil)
    }

    @Test func embyAndJellyfinSignInHeadersDiffer() {
        let jellyfin = Emby.headers(device: "dev-1", kind: .jellyfin, token: "t")
        #expect(jellyfin.count == 1 && jellyfin[0].0 == "Authorization")
        #expect(jellyfin[0].1.contains("DeviceId=\"dev-1\"") && jellyfin[0].1.contains("Token=\"t\""))
        let emby = Emby.headers(device: "dev-1", kind: .emby, token: "t")
        #expect(emby[0].0 == "X-Emby-Authorization")
        #expect(emby[1] == ("X-Emby-Token", "t"))
    }

    // MARK: Against a stand-in server

    @Test func signingInKeepsTheTokenAndNamesTheServer() async throws {
        FixtureServer.answer("http://jf1.test:8096/Users/AuthenticateByName", #"{"AccessToken": "tok", "User": {"Id": "u9"}}"#)
        FixtureServer.answer("http://jf1.test:8096/System/Info/Public", #"{"ServerName": "Den"}"#)
        let server = try await stubClient().signIn(.jellyfin, address: "jf1.test:8096/", user: "me", password: "pw")
        #expect(server.name == "Den" && server.token == "tok" && server.userID == "u9" && server.address == "http://jf1.test:8096")
        FixtureServer.answer("http://jf2.test/Users/AuthenticateByName", status: 401, "")
        await #expect(throws: ServerError.signInRefused) {
            _ = try await stubClient().signIn(.emby, address: "http://jf2.test", user: "me", password: "bad")
        }
    }

    @Test func aPublicServerIsRefusedInTheAppStoreBuild() async {
        var client = stubClient()
        client.ownNetworkOnly = true
        await #expect(throws: ServerError.notOwnNetwork) {
            _ = try await client.call("http://8.8.8.8/System/Info/Public", headers: [])
        }
    }

    @Test func aLongListOfSourcesArrivesInBatches() async throws {
        let server = Server(id: 3, kind: .jellyfin, name: "Big", address: "http://big.test", userID: "u", token: "t")
        let sources = (1 ... 25).map { #"{"Id": "s\#($0)", "Name": "Source \#($0)", "Protocol": "Http", "Path": "https://x.example/\#($0).mkv"}"# }
        FixtureServer.answer(
            "http://big.test/Items/m1/PlaybackInfo?UserId=u&StartTimeTicks=0&IsPlayback=false&AutoOpenLiveStream=false&MaxStreamingBitrate=1000000000",
            #"{"MediaSources": [\#(sources.joined(separator: ",")), {"Id": "s1", "Name": "repeat"}]}"#)
        var batches: [Int] = []
        try await stubClient().streamsLive(server, item: "m1") { some in
            batches.append(some.count)
            return true
        }
        #expect(batches == [10, 10, 5], "a repeated source is listed once")
        // Asked to stop after the first batch: no more.
        var taken = 0
        try await stubClient().streamsLive(server, item: "m1") { some in
            taken += some.count
            return false
        }
        #expect(taken == 10)
    }

    @Test func aServerWithoutPlaybackInformationOffersTheItemsFiles() async throws {
        let server = Server(id: 4, kind: .emby, name: "Old", address: "http://old.test", userID: "u", token: "t")
        FixtureServer.answer(
            "http://old.test/Items/m1/PlaybackInfo?UserId=u&StartTimeTicks=0&IsPlayback=false&AutoOpenLiveStream=false&MaxStreamingBitrate=1000000000",
            status: 500, "")
        FixtureServer.answer("http://old.test/Users/u/Items/m1?Fields=MediaSources,MediaStreams",
                             #"{"MediaSources": [{"Id": "v1", "Container": "mkv"}]}"#)
        let streams = try await stubClient().streams(server, item: "m1")
        #expect(streams.map(\.url) == ["http://old.test/Videos/m1/stream?static=true&MediaSourceId=v1&api_key=t"])
        // An item that says it has no sources has none.
        FixtureServer.answer(
            "http://old.test/Items/m2/PlaybackInfo?UserId=u&StartTimeTicks=0&IsPlayback=false&AutoOpenLiveStream=false&MaxStreamingBitrate=1000000000",
            status: 500, "")
        FixtureServer.answer("http://old.test/Users/u/Items/m2?Fields=MediaSources,MediaStreams", #"{"MediaSources": []}"#)
        #expect(try await stubClient().streams(server, item: "m2").isEmpty)
    }

    @Test func anEpisodeIsFoundOnAnotherServerByItsSeries() async throws {
        let server = Server(id: 5, kind: .jellyfin, name: "Find", address: "http://find.test", userID: "u", token: "t")
        FixtureServer.answer("http://find.test/Users/u/Items?SearchTerm=A%20Show&Recursive=true&IncludeItemTypes=Series&Fields=ProviderIds&Limit=25",
                             #"{"Items": [{"Id": "s1", "Name": "A Show", "ProviderIds": {"Imdb": "tt5"}}]}"#)
        FixtureServer.answer("http://find.test/Shows/s1/Episodes?UserId=u",
                             #"{"Items": [{"Id": "e9", "ParentIndexNumber": 2, "IndexNumber": 3}]}"#)
        let wanted = Wanted(series: true, name: " A Show ", imdb: "tt5", episode: (2, 3))
        #expect(try await stubClient().find(server, wanted) == "e9")
        var whole = wanted
        whole.episode = nil
        #expect(try await stubClient().find(server, whole) == nil, "a series without an episode is nothing to play")
    }
}

/// The Rust build's Plex tests (haven-servers plex.rs), ported.
struct PlexTests {
    private let server = Server(id: 5, kind: .plex, name: "Den", address: "https://plex.example:32400", token: "tk")

    @Test func resourcesKeepServersAndOrderTheirAddresses() {
        let resources = json("""
            [{"name": "Den", "provides": "server", "accessToken": "a", "connections": [
                {"uri": "https://relay.example/", "relay": true, "local": false},
                {"uri": "https://remote.example", "relay": false, "local": false},
                {"uri": "http://192.168.1.9:32400", "relay": false, "local": true}]},
             {"name": "Phone", "provides": "client,player", "accessToken": "b", "connections": []},
             {"name": "No token", "provides": "server"}]
            """)
        let listed = Plex.readResources(resources)
        #expect(listed.count == 1)
        #expect(listed[0].addresses == ["http://192.168.1.9:32400", "https://remote.example", "https://relay.example"])
    }

    @Test func librariesAndPreviews() {
        let sections = json(#"{"Directory": [{"key": "1", "title": "Films", "type": "movie"}, {"key": "2", "title": "TV", "type": "show"}, {"key": "3", "title": "Music", "type": "artist"}]}"#)
        #expect(Plex.readLibraries(sections).map(\.name) == ["Films", "TV"])
        let recent = json("""
            {"Metadata": [
              {"type": "movie", "ratingKey": "10", "title": "A Film", "year": 2021, "thumb": "/library/metadata/10/thumb/1"},
              {"type": "season", "ratingKey": "21", "parentRatingKey": "20", "parentTitle": "A Show", "parentThumb": "/t/20"},
              {"type": "episode", "ratingKey": "22", "grandparentRatingKey": "20", "grandparentTitle": "A Show"},
              {"type": "track", "ratingKey": "30", "title": "A Song"}]}
            """)
        let previews = Plex.readPreviews(server, recent)
        #expect(previews.map { "\($0.id) \($0.name)" } == ["srv:5:10 A Film", "srv:5:20 A Show"])
        #expect(previews[0].poster == "https://plex.example:32400/library/metadata/10/thumb/1?X-Plex-Token=tk")
    }

    @Test func detailAndStreams() throws {
        let detail = json("""
            {"Metadata": [{"type": "movie", "ratingKey": "10", "title": "A Film", "summary": "About it.",
              "year": 2021, "duration": 5400000, "rating": 7.44,
              "Genre": [{"tag": "Drama"}], "Role": [{"tag": "An Actor"}], "Director": [{"tag": "A Director"}],
              "Media": [
                {"videoResolution": "1080", "container": "mkv", "videoCodec": "h264", "audioCodec": "aac", "Part": [{"key": "/library/parts/7/file.mkv"}]},
                {"videoResolution": "4k", "Part": [{"key": "/library/parts/8/file.mkv"}]},
                {"videoResolution": "sd", "Part": []}]}]}
            """)
        let meta = try #require(Plex.readDetail(server, detail, episodes: NSNull()))
        #expect(meta.kind == "movie" && meta.runtime == "90 min" && meta.rating == "7.4")
        #expect(meta.cast == ["An Actor"])
        let streams = Plex.readStreams(server, detail)
        #expect(streams.count == 2)
        #expect(streams[0].name == "1080p")
        #expect(streams[0].description == "MKV · H264 · AAC")
        #expect(streams[1].name == "4K")
        #expect(streams[0].url == "https://plex.example:32400/library/parts/7/file.mkv?X-Plex-Token=tk")
    }

    @Test func candidatesReadIDsFromBothKindsOfGuid() {
        let found = json("""
            {"Metadata": [
              {"ratingKey": "10", "title": "A Film", "year": 2021, "Guid": [{"id": "imdb://tt0000010"}, {"id": "tmdb://77"}]},
              {"ratingKey": "11", "title": "Old Agent", "guid": "com.plexapp.agents.imdb://tt0000011?lang=en"},
              {"ratingKey": "12", "title": "Bare", "guid": "plex://movie/abc"}]}
            """)
        let candidates = Plex.readCandidates(found)
        #expect(candidates[0].imdb == "tt0000010" && candidates[0].tmdb == "77")
        #expect(candidates[1].imdb == "tt0000011")
        #expect(candidates[2].imdb == nil && candidates[2].tmdb == nil)
        let leaves = json(#"{"Metadata": [{"ratingKey": "22", "parentIndex": 1, "index": 1}, {"ratingKey": "23", "parentIndex": 1, "index": 2}]}"#)
        #expect(Plex.readEpisode(leaves, season: 1, number: 2) == "23")
        #expect(Plex.readEpisode(leaves, season: 2, number: 2) == nil)
    }

    @Test func aShowListsItsEpisodes() throws {
        let detail = json(#"{"Metadata": [{"type": "show", "ratingKey": "20", "title": "A Show"}]}"#)
        let leaves = json(#"{"Metadata": [{"ratingKey": "22", "title": "Pilot", "parentIndex": 1, "index": 1, "originallyAvailableAt": "2020-02-02"}]}"#)
        let meta = try #require(Plex.readDetail(server, detail, episodes: leaves))
        #expect(meta.kind == "series")
        #expect(meta.videos[0].id == "srv:5:22")
        #expect(meta.videos[0].released == "2020-02-02")
    }

    @Test func thePlexCodeLeadsToTheAccountsServers() async throws {
        let client = stubClient()
        FixtureServer.answer("https://plex.tv/api/v2/pins?strong=true", #"{"id": 77, "code": "ABCD"}"#)
        FixtureServer.answer("https://plex.tv/api/v2/pins/77?code=ABCD", #"{"authToken": "acct"}"#)
        FixtureServer.answer("https://plex.tv/api/v2/resources?includeHttps=1&includeRelay=1", """
            [{"name": "Den", "provides": "server", "accessToken": "srv", "connections": [
               {"uri": "http://den-local.test:32400", "local": true}, {"uri": "https://den-remote.test", "local": false}]}]
            """)
        // The local address does not answer; the remote one does.
        FixtureServer.answer("https://den-remote.test/identity", #"{"MediaContainer": {}}"#)
        nonisolated(unsafe) var shown: Plex.Pin?
        let outcome = await ServerAccounts.plexSignIn(client: client) { pin in shown = pin }
        #expect(shown?.code == "ABCD")
        #expect(shown?.link.hasPrefix("https://app.plex.tv/auth#?clientID=dev-1&code=ABCD") == true)
        guard case let .servers(servers) = outcome else {
            Issue.record("no servers: \(String(describing: outcome))")
            return
        }
        #expect(servers.map(\.address) == ["https://den-remote.test"])
        #expect(servers[0].token == "srv" && servers[0].kind == .plex)
    }
}
