package net.haven.app.servers

import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import net.haven.app.core.DeviceId
import net.haven.app.core.StoredServer
import net.haven.app.core.Store
import net.haven.app.testing.FakeWeb
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

private fun json(text: String): JsonElement = Json.parseToJsonElement(text)
private fun hex(text: String) = Servers.hex(text)

/** The Rust build's shared server tests (haven-servers lib.rs), ported. */
class ServerRulesTest {
    @Test
    fun idsRoundTripAndOtherIdsAreLeftAlone() {
        assertEquals("srv:3:abc", Servers.titleId(3, "abc"))
        assertEquals(3L to "abc", Servers.splitId("srv:3:abc"))
        assertEquals(12L to "a:b", Servers.splitId("srv:12:a:b"))
        assertNull(Servers.splitId("tt0903747"))
        assertNull(Servers.splitId("srv:x:abc"))
        assertNull(Servers.splitId("srv:3:"))
    }

    @Test
    fun addressesAreTidied() {
        assertEquals("http://192.168.1.5:8096", Servers.address(" 192.168.1.5:8096/ "))
        assertEquals("https://media.example.org/jf", Servers.address("https://media.example.org/jf/"))
        assertThrows(ServerError::class.java) { Servers.address("ftp://x") }
        assertThrows(ServerError::class.java) { Servers.address("") }
    }

    @Test
    fun theWantedTitleIsMatchedByIdThenByNameAndYear() {
        val onServer = listOf(
            Candidate("a", "The Film", "1990", "tt1"),
            Candidate("b", "The Film", "2021", "tt2"),
            Candidate("c", "Another: Film!", "2005", null),
        )
        fun wanted(name: String, year: String, imdb: String?) = Wanted(name = name, year = year, imdb = imdb)
        // The id decides, whatever the name says.
        assertEquals("b", Servers.bestMatch(wanted("Renamed", "2021", "TT2"), onServer)?.id)
        // A different id of the same kind is another title, even by the same name.
        assertNull(Servers.bestMatch(wanted("The Film", "1990", "tt9"), onServer))
        // No id to compare: name and year.
        assertEquals("c", Servers.bestMatch(wanted("another film", "2005-2009", null), onServer)?.id)
        assertNull(Servers.bestMatch(wanted("another film", "2006", null), onServer))
        assertNull(Servers.bestMatch(wanted("", "2005", null), onServer))
    }

    @Test
    fun sizesAndQualitiesReadWell() {
        assertEquals("4K", Servers.quality(3840.0, 1600.0))
        assertEquals("1080p", Servers.quality(1920.0, 800.0))
        assertEquals("720p", Servers.quality(null, 720.0))
        assertEquals("576p", Servers.quality(720.0, 576.0))
        assertEquals("SD", Servers.quality(640.0, null))
        assertNull(Servers.quality(null, null))
        assertEquals("4.5 GB", Servers.size(4.5 * 1024 * 1024 * 1024))
        assertEquals("700 MB", Servers.size(700.0 * 1024 * 1024))
        assertNull(Servers.size(10.0))
    }

    @Test
    fun queryTextIsEscaped() {
        assertEquals("the%20matrix%20%26%20more", Servers.query("the matrix & more"))
    }

    @Test
    fun aServerNeverPrintsItsToken() {
        assertFalse(Server(1, ServerKind.Plex, "Home", "http://x", token = "topsecret").toString().contains("topsecret"))
    }

    @Test
    fun storedServersComeBackAsServersAndUnknownKindsAreSkipped() {
        val store = Store.inMemory()
        val a = store.saveServer(StoredServer(kind = "jellyfin", name = "A", address = "http://a", userId = "u", token = "t"))
        store.saveServer(StoredServer(kind = "jukebox", name = "Odd", address = "http://o", userId = "", token = "t"))
        val b = store.saveServer(StoredServer(kind = "plex", name = "B", address = "http://b", userId = "", token = "t"))
        assertEquals(listOf("A", "B"), ServerAccounts.load(store).all.map { it.name })
        assertEquals(ServerAccounts.Status("Server switched off.", false), ServerAccounts.setEnabled(a, on = false, store = store))
        assertEquals(listOf(b), ServerAccounts.load(store).enabled.map { it.id })
        assertEquals("42", ServerAccounts.serverItem("srv:$b:42", ServerAccounts.load(store).enabled)?.second)
        assertNull(ServerAccounts.serverItem("srv:$a:42", ServerAccounts.load(store).enabled))
        assertEquals("Server removed. Haven has forgotten its sign-in.", ServerAccounts.remove(b, store).text)
        assertEquals(listOf("A"), ServerAccounts.load(store).all.map { it.name })
    }

    @Test
    fun libraryShelvesAreNewAndByName() {
        val server = Server(2, ServerKind.Jellyfin, "Home", "http://h", token = "t")
        val plans = ServerAccounts.libraryPlans(
            listOf(server to listOf(MediaLibrary("1", "Films", false), MediaLibrary("c", "Popular films", true))),
        )
        assertEquals(listOf("New in Films", "Films", "Popular films"), plans.map { it.title })
        assertEquals(listOf(true, false, false), plans.map { it.latest })
        assertTrue(plans.all { it.source == "Home" })
        assertEquals("libraries|2|http://h", ServerAccounts.librariesName(server))
    }

    @Test
    fun theDeviceIdIsMadeOnceAndKept() {
        val store = Store.inMemory()
        val id = DeviceId.get(store)
        assertTrue(id.startsWith("haven-") && id.length == 38)
        assertEquals(id, DeviceId.get(store))
        store.put(DeviceId.KEY, "haven-fromrust")
        assertEquals("haven-fromrust", DeviceId.get(store))
    }
}

/** The Rust build's Jellyfin and Emby tests (haven-servers emby.rs), ported. */
class EmbyTest {
    private val server = Server(2, ServerKind.Jellyfin, "Home", "http://media.example", "u1", "tok en")

    @Test
    fun librariesKeepFilmsAndSeriesOnly() {
        val views = json(
            """{"Items": [{"Id": "a", "Name": "Films", "CollectionType": "movies"},
                       {"Id": "b", "Name": "Shows", "CollectionType": "tvshows"},
                       {"Id": "c", "Name": "Music", "CollectionType": "music"},
                       {"Id": "d", "Name": "Mixed"},
                       {"Name": "No id", "CollectionType": "movies"}]}""",
        )
        assertEquals(listOf("Films", "Shows", "Mixed"), Emby.readLibraries(views).map { it.name })
    }

    @Test
    fun collectionsBecomeLibraries() {
        val views = json("""{"Items": [{"Id": "a", "Name": "Movies", "CollectionType": "movies"}, {"Id": "636f6c", "Name": "Collections", "CollectionType": "boxsets"}]}""")
        assertEquals(listOf("636f6c"), Emby.collectionViews(views))
        assertEquals(1, Emby.readLibraries(views).size)
        val found = json("""{"Items": [{"Id": "c1", "Name": "Popular films", "Type": "BoxSet"}, {"Id": "m1", "Name": "A Film", "Type": "Movie"}, {"Name": "No id", "Type": "BoxSet"}]}""")
        assertEquals(listOf(MediaLibrary("c1", "Popular films", true)), Emby.readCollections(found))
    }

    @Test
    fun sharedAddonsAreRead() {
        val config = json(
            """{"addons": [{"name": "One", "manifestUrl": "https://one.example/key123/manifest.json"},
                        {"name": "Two", "nameOverride": "Second", "manifestUrl": "https://two.example/manifest.json"},
                        {"name": "Off", "manifestUrl": "https://off.example/manifest.json", "disabled": true},
                        {"name": "No address"}, {"name": "Odd", "manifestUrl": "file:///x"}]}""",
        )
        val addons = Emby.readSharedAddons(config)
        assertEquals(listOf("One", "Second"), addons.map { it.name })
        assertFalse(addons[0].toString().contains("key123"))
        assertTrue(Emby.readSharedAddons(json("{}")).isEmpty())
    }

    @Test
    fun sourcesArePickedOutAsTheAnswerArrives() {
        val answer = """{"Note":"MediaSources","Other":[{"Id":"no"}],"MediaSources":[{"Id":"a","Name":"One \"quoted\" } ]","MediaStreams":[{"Type":"Subtitle"}]},{"Id":"b","Nested":{"MediaSources":[{"Id":"inner"}]}}],"PlaySessionId":"x"}"""
        fun ids(entries: List<JsonElement>) = entries.mapNotNull { SJ.text(it, "Id") }
        val bytes = answer.toByteArray()
        // Whole, and a few bytes at a time: the same two entries.
        val whole = SourceScanner()
        assertEquals(listOf("a", "b"), ids(whole.feed(bytes)))
        assertTrue(whole.done)
        val slow = SourceScanner()
        val entries = mutableListOf<JsonElement>()
        for (start in bytes.indices step 7) entries += slow.feed(bytes, start, minOf(start + 7, bytes.size))
        assertEquals(listOf("a", "b"), ids(entries))
        assertEquals("One \"quoted\" } ]", SJ.text(entries[0], "Name"))
        // The first is there before the answer is complete.
        val early = SourceScanner()
        val cut = answer.indexOf("\"Id\":\"b\"")
        assertEquals(listOf("a"), ids(early.feed(answer.substring(0, cut).toByteArray())))
        assertFalse(early.done)
    }

    @Test
    fun aGatheredTitleCanBeNamedByItsImdbId() {
        val byTmdb = hex("m|tmdb:603|movie")
        assertTrue(Servers.wantsImdb(byTmdb))
        assertEquals(hex("m|tt0133093|movie"), Servers.byImdb(byTmdb, "tt0133093"))
        assertEquals(hex("s|tt0944947|series"), Servers.byImdb(hex("s|kitsu:12|series"), "tt0944947"))
        assertFalse(Servers.wantsImdb(hex("m|tt0133093|movie")))
        assertNull(Servers.byImdb(hex("m|tt0133093|movie"), "tt0133093"))
        assertFalse(Servers.wantsImdb("f27caa37e5142225cceded48f6553502"))
        assertNull(Servers.byImdb("f27caa37e5142225cceded48f6553502", "tt0133093"))
        assertNull(Servers.byImdb(byTmdb, "603"))
    }

    @Test
    fun anErrorPassedOnIsNotAFilm() {
        val error = hex("m|aiostreamserror.%7B%22errorDescription%22%3A%22Rate%20Limit%22%7D|movie")
        assertTrue(Emby.isErrorEntry(error))
        assertFalse(Emby.isErrorEntry(hex("m|tt0111161|movie")))
        assertFalse(Emby.isErrorEntry("f27caa37e5142225cceded48f6553502"))
        val items = json(
            """[{"Type": "Movie", "Id": "$error", "Name": "Rate Limit Exceeded"},
             {"Type": "Movie", "Id": "${hex("m|tt0111161|movie")}", "Name": "A Film"}]""",
        ) as JsonArray
        assertEquals(1, Emby.readPreviews(server, items).size)
    }

    @Test
    fun previewsFoldEpisodesIntoTheirSeries() {
        val items = json(
            """[{"Type": "Movie", "Id": "m1", "Name": "A Film", "ProductionYear": 2020},
             {"Type": "Episode", "Id": "e1", "Name": "Pilot", "SeriesId": "s1", "SeriesName": "A Show"},
             {"Type": "Episode", "Id": "e2", "Name": "Two", "SeriesId": "s1", "SeriesName": "A Show"},
             {"Type": "Series", "Id": "s2", "Name": "Another Show"},
             {"Type": "Audio", "Id": "x", "Name": "A Song"}]""",
        ) as JsonArray
        val previews = Emby.readPreviews(server, items)
        assertEquals(
            listOf("srv:2:m1 movie A Film", "srv:2:s1 series A Show", "srv:2:s2 series Another Show"),
            previews.map { "${it.id} ${it.kind} ${it.name}" },
        )
        assertEquals("2020", previews[0].releaseInfo)
        assertEquals("http://media.example/Items/m1/Images/Primary?maxHeight=450&quality=90&api_key=tok%20en", previews[0].poster)
    }

    @Test
    fun detailOfASeriesListsItsEpisodes() {
        val detail = json(
            """{"Id": "s1", "Type": "Series", "Name": "A Show", "Overview": "About it.",
             "ProductionYear": 2019, "CommunityRating": 8.25, "Genres": ["Drama", "Crime"],
             "RunTimeTicks": 27000000000, "BackdropImageTags": ["x"],
             "People": [{"Name": "An Actor", "Type": "Actor"}, {"Name": "A Director", "Type": "Director"}],
             "ProviderIds": {"Imdb": "tt123", "Tmdb": "456"}}""",
        )
        val episodes = json("""{"Items": [{"Id": "e1", "Name": "Pilot", "ParentIndexNumber": 1, "IndexNumber": 1, "PremiereDate": "2019-01-01T00:00:00Z"}, {"Name": "No id"}]}""")
        val meta = Emby.readDetail(server, detail, episodes)!!
        assertTrue(meta.id == "srv:2:s1" && meta.kind == "series")
        assertEquals("45 min", meta.runtime)
        assertEquals("8.2", meta.rating)
        assertEquals(listOf("Drama", "Crime"), meta.genres)
        assertTrue(meta.cast == listOf("An Actor") && meta.director == listOf("A Director"))
        assertNotNull(meta.background)
        assertTrue(meta.videos.size == 1 && meta.videos[0].id == "srv:2:e1")
        assertTrue(meta.imdbId == "tt123" && meta.tmdbId == "456")
        assertTrue(meta.videos[0].season == 1 && meta.videos[0].episode == 1)
    }

    @Test
    fun theStreamAddressCarriesTheEscapedToken() {
        val streams = Emby.readStreams(server, "m1", json("{}"))
        assertEquals(1, streams.size)
        assertEquals("http://media.example/Videos/m1/stream?static=true&api_key=tok%20en", streams[0].url)
    }

    @Test
    fun everyVersionOfAFileIsAStream() {
        val detail = json(
            """{"MediaSources": [
              {"Id": "v1", "Name": "4K HDR", "Container": "mkv", "Size": 21474836480, "MediaStreams": [
                {"Type": "Video", "Codec": "hevc", "Width": 3840, "Height": 2160}, {"Type": "Audio", "Codec": "eac3"}]},
              {"Id": "v2", "Name": "HD", "Container": "mp4", "MediaStreams": [{"Type": "Video", "Codec": "h264", "Width": 1920, "Height": 1080}]},
              {"Name": "No id"}]}""",
        )
        val streams = Emby.readStreams(server, "m1", detail)
        assertEquals(2, streams.size)
        assertEquals("4K", streams[0].name)
        assertEquals("4K HDR · MKV · HEVC · EAC3 · 20.0 GB", streams[0].description)
        assertEquals("http://media.example/Videos/m1/stream?static=true&MediaSourceId=v1&api_key=tok%20en", streams[0].url)
        assertEquals("1080p", streams[1].name)
        assertEquals("HD · MP4 · H264", streams[1].description)
    }

    @Test
    fun gatheredStreamsPlayFromTheirOwnAddress() {
        val info = json(
            """{"MediaSources": [
              {"Id": "a", "Name": "Provider One 1080p WEB", "Protocol": "Http", "IsRemote": true,
               "Path": "https://streams.example/one.mkv", "Container": "mkv", "Remux": {"ProviderInfo": {"source": "Provider One"}}},
              {"Name": "Provider Two 4K", "Protocol": "Http", "Path": "https://streams.example/two.mp4",
               "MediaStreams": [{"Type": "Video", "Codec": "hevc", "Width": 3840, "Height": 2160}]},
              {"Id": "c", "Name": "A file", "Protocol": "File", "Path": "/media/file.mkv"},
              {"Name": "Nothing to play", "Protocol": "File", "Path": "/media/x.mkv"}]}""",
        )
        val streams = Emby.readStreams(server, "m1", info)
        assertEquals(3, streams.size)
        assertEquals("https://streams.example/one.mkv", streams[0].url)
        assertEquals("Provider One 1080p WEB", streams[0].name)
        assertEquals("MKV", streams[0].description)
        assertEquals("4K", streams[1].name)
        assertEquals("https://streams.example/two.mp4", streams[1].url)
        assertEquals("Provider One", streams[0].provider)
        assertNull(streams[1].provider)
        assertEquals("http://media.example/Videos/m1/stream?static=true&MediaSourceId=c&api_key=tok%20en", streams[2].url)
    }

    @Test
    fun aSourceCanNameItsOwnAddressAndTheLogNamesNoSecrets() {
        val info = json(
            """{"MediaSources": [{"Id": "a", "Name": "Secret name", "Protocol": "File", "Path": "/media/secret.mkv",
                               "DirectStreamUrl": "/Videos/m1/stream.mkv?MediaSourceId=a"}], "ErrorCode": null}""",
        )
        assertEquals("http://media.example/Videos/m1/stream.mkv?MediaSourceId=a&api_key=tok%20en", Emby.readStreams(server, "m1", info)[0].url)
        val shape = Emby.describeSources(info)
        assertTrue(shape, shape.startsWith("lists 1 [protocol File id yes name yes path yes url yes streams 0]"))
        assertFalse(shape.contains("secret") || shape.contains("Secret"))
        assertTrue(Emby.describeSources(json("{}")).startsWith("lists 0"))
    }

    @Test
    fun picturesCarryTheirTagAndRemoteOnesAreUsedAsGiven() {
        val detail = json("""{"Id": "m1", "Type": "Movie", "Name": "A Film", "ImageTags": {"Primary": "p1"}, "BackdropImageTags": ["b1"]}""")
        val meta = Emby.readDetail(server, detail, JsonNull)!!
        assertEquals("http://media.example/Items/m1/Images/Primary?maxHeight=450&quality=90&tag=p1&api_key=tok%20en", meta.poster)
        assertEquals("http://media.example/Items/m1/Images/Backdrop?maxHeight=900&quality=90&tag=b1&api_key=tok%20en", meta.background)
        val gathered = json("""{"Id": "m2", "Type": "Movie", "Name": "Gathered", "RemoteImages": [{"Type": "Primary", "Url": "https://pictures.example/p.jpg"}, {"Type": "Backdrop", "Url": "file:///x"}]}""")
        val other = Emby.readDetail(server, gathered, JsonNull)!!
        assertEquals("https://pictures.example/p.jpg", other.poster)
        assertTrue(other.background!!.contains("/Items/m2/Images/Backdrop"))
    }

    @Test
    fun candidatesCarryTheirIdsAndEpisodesAreFoundByNumber() {
        val items = json("""{"Items": [{"Id": "m1", "Name": "A Film", "ProductionYear": 2020, "ProviderIds": {"Imdb": "tt7", "Tmdb": "42"}}, {"Id": "m2", "Name": "Bare", "ProviderIds": {"imdb": " "}}, {"Name": "No id"}]}""")
        val candidates = Emby.readCandidates(items)
        assertEquals(2, candidates.size)
        assertTrue(candidates[0].imdb == "tt7" && candidates[0].tmdb == "42" && candidates[0].year == "2020")
        assertNull(candidates[1].imdb)
        val episodes = json("""{"Items": [{"Id": "e1", "ParentIndexNumber": 1, "IndexNumber": 1}, {"Id": "e2", "ParentIndexNumber": 1, "IndexNumber": 2}, {"Id": "e3", "ParentIndexNumber": 2, "IndexNumber": 1}]}""")
        assertEquals("e3", Emby.readEpisode(episodes, 2, 1))
        assertNull(Emby.readEpisode(episodes, 3, 1))
    }

    @Test
    fun embyAndJellyfinSignInHeadersDiffer() {
        val jellyfin = Emby.headers("dev-1", ServerKind.Jellyfin, "t")
        assertTrue(jellyfin.size == 1 && jellyfin[0].first == "Authorization")
        assertTrue(jellyfin[0].second.contains("DeviceId=\"dev-1\"") && jellyfin[0].second.contains("Token=\"t\""))
        val emby = Emby.headers("dev-1", ServerKind.Emby, "t")
        assertEquals("X-Emby-Authorization", emby[0].first)
        assertEquals("X-Emby-Token" to "t", emby[1])
    }

    // Against a stand-in server

    @Test
    fun signingInKeepsTheTokenAndNamesTheServer() = runBlocking {
        val web = FakeWeb().apply {
            answer("http://jf1.test:8096/Users/AuthenticateByName", """{"AccessToken": "tok", "User": {"Id": "u9"}}""")
            answer("http://jf1.test:8096/System/Info/Public", """{"ServerName": "Den"}""")
            answer("http://jf2.test/Users/AuthenticateByName", "", status = 401)
        }
        val client = ServerClient("dev-1", web.http)
        val signed = client.signIn(ServerKind.Jellyfin, "jf1.test:8096/", "me", "pw")
        assertTrue(signed.name == "Den" && signed.token == "tok" && signed.userId == "u9" && signed.address == "http://jf1.test:8096")
        assertEquals(ServerError.SignInRefused(), runCatching { client.signIn(ServerKind.Emby, "http://jf2.test", "me", "bad") }.exceptionOrNull())
    }

    private val playbackPath = "/PlaybackInfo?UserId=u&StartTimeTicks=0&IsPlayback=false&AutoOpenLiveStream=false&MaxStreamingBitrate=1000000000"

    @Test
    fun aLongListOfSourcesArrivesInBatches() = runBlocking {
        val big = Server(3, ServerKind.Jellyfin, "Big", "http://big.test", "u", "t")
        val sources = (1..25).map { """{"Id": "s$it", "Name": "Source $it", "Protocol": "Http", "Path": "https://x.example/$it.mkv"}""" }
        val web = FakeWeb().apply {
            answer("http://big.test/Items/m1$playbackPath", """{"MediaSources": [${sources.joinToString(",")}, {"Id": "s1", "Name": "repeat"}]}""")
        }
        val client = ServerClient("dev-1", web.http)
        val batches = mutableListOf<Int>()
        client.streamsLive(big, "m1") { batches += it.size; true }
        assertEquals("a repeated source is listed once", listOf(10, 10, 5), batches)
        // Asked to stop after the first batch: no more.
        var taken = 0
        client.streamsLive(big, "m1") { taken += it.size; false }
        assertEquals(10, taken)
    }

    @Test
    fun aServerWithoutPlaybackInformationOffersTheItemsFiles() = runBlocking {
        val old = Server(4, ServerKind.Emby, "Old", "http://old.test", "u", "t")
        val web = FakeWeb().apply {
            answer("http://old.test/Items/m1$playbackPath", "", status = 500)
            answer("http://old.test/Users/u/Items/m1?Fields=MediaSources,MediaStreams", """{"MediaSources": [{"Id": "v1", "Container": "mkv"}]}""")
            answer("http://old.test/Items/m2$playbackPath", "", status = 500)
            answer("http://old.test/Users/u/Items/m2?Fields=MediaSources,MediaStreams", """{"MediaSources": []}""")
        }
        val client = ServerClient("dev-1", web.http)
        assertEquals(listOf("http://old.test/Videos/m1/stream?static=true&MediaSourceId=v1&api_key=t"), client.streams(old, "m1").map { it.url })
        // An item that says it has no sources has none.
        assertTrue(client.streams(old, "m2").isEmpty())
    }

    @Test
    fun anEpisodeIsFoundOnAnotherServerByItsSeries() = runBlocking {
        val find = Server(5, ServerKind.Jellyfin, "Find", "http://find.test", "u", "t")
        val web = FakeWeb().apply {
            answer(
                "http://find.test/Users/u/Items?SearchTerm=A%20Show&Recursive=true&IncludeItemTypes=Series&Fields=ProviderIds&Limit=25",
                """{"Items": [{"Id": "s1", "Name": "A Show", "ProviderIds": {"Imdb": "tt5"}}]}""",
            )
            answer("http://find.test/Shows/s1/Episodes?UserId=u", """{"Items": [{"Id": "e9", "ParentIndexNumber": 2, "IndexNumber": 3}]}""")
        }
        val client = ServerClient("dev-1", web.http)
        val wanted = Wanted(series = true, name = " A Show ", imdb = "tt5", episode = 2 to 3)
        assertEquals("e9", client.find(find, wanted))
        assertNull("a series without an episode is nothing to play", client.find(find, wanted.copy(episode = null)))
    }
}

/** The Rust build's Plex tests (haven-servers plex.rs), ported. */
class PlexTest {
    private val server = Server(5, ServerKind.Plex, "Den", "https://plex.example:32400", token = "tk")

    @Test
    fun resourcesKeepServersAndOrderTheirAddresses() {
        val listed = Plex.readResources(
            json(
                """[{"name": "Den", "provides": "server", "accessToken": "a", "connections": [
                {"uri": "https://relay.example/", "relay": true, "local": false},
                {"uri": "https://remote.example", "relay": false, "local": false},
                {"uri": "http://192.168.1.9:32400", "relay": false, "local": true}]},
             {"name": "Phone", "provides": "client,player", "accessToken": "b", "connections": []},
             {"name": "No token", "provides": "server"}]""",
            ),
        )
        assertEquals(1, listed.size)
        assertEquals(listOf("http://192.168.1.9:32400", "https://remote.example", "https://relay.example"), listed[0].addresses)
    }

    @Test
    fun librariesAndPreviews() {
        val sections = json("""{"Directory": [{"key": "1", "title": "Films", "type": "movie"}, {"key": "2", "title": "TV", "type": "show"}, {"key": "3", "title": "Music", "type": "artist"}]}""")
        assertEquals(listOf("Films", "TV"), Plex.readLibraries(sections).map { it.name })
        val recent = json(
            """{"Metadata": [
              {"type": "movie", "ratingKey": "10", "title": "A Film", "year": 2021, "thumb": "/library/metadata/10/thumb/1"},
              {"type": "season", "ratingKey": "21", "parentRatingKey": "20", "parentTitle": "A Show", "parentThumb": "/t/20"},
              {"type": "episode", "ratingKey": "22", "grandparentRatingKey": "20", "grandparentTitle": "A Show"},
              {"type": "track", "ratingKey": "30", "title": "A Song"}]}""",
        )
        val previews = Plex.readPreviews(server, recent)
        assertEquals(listOf("srv:5:10 A Film", "srv:5:20 A Show"), previews.map { "${it.id} ${it.name}" })
        assertEquals("https://plex.example:32400/library/metadata/10/thumb/1?X-Plex-Token=tk", previews[0].poster)
    }

    @Test
    fun detailAndStreams() {
        val detail = json(
            """{"Metadata": [{"type": "movie", "ratingKey": "10", "title": "A Film", "summary": "About it.",
              "year": 2021, "duration": 5400000, "rating": 7.44,
              "Genre": [{"tag": "Drama"}], "Role": [{"tag": "An Actor"}], "Director": [{"tag": "A Director"}],
              "Media": [
                {"videoResolution": "1080", "container": "mkv", "videoCodec": "h264", "audioCodec": "aac", "Part": [{"key": "/library/parts/7/file.mkv"}]},
                {"videoResolution": "4k", "Part": [{"key": "/library/parts/8/file.mkv"}]},
                {"videoResolution": "sd", "Part": []}]}]}""",
        )
        val meta = Plex.readDetail(server, detail, JsonNull)!!
        assertTrue(meta.kind == "movie" && meta.runtime == "90 min" && meta.rating == "7.4")
        assertEquals(listOf("An Actor"), meta.cast)
        val streams = Plex.readStreams(server, detail)
        assertEquals(2, streams.size)
        assertEquals("1080p", streams[0].name)
        assertEquals("MKV · H264 · AAC", streams[0].description)
        assertEquals("4K", streams[1].name)
        assertEquals("https://plex.example:32400/library/parts/7/file.mkv?X-Plex-Token=tk", streams[0].url)
    }

    @Test
    fun candidatesReadIdsFromBothKindsOfGuid() {
        val candidates = Plex.readCandidates(
            json(
                """{"Metadata": [
              {"ratingKey": "10", "title": "A Film", "year": 2021, "Guid": [{"id": "imdb://tt0000010"}, {"id": "tmdb://77"}]},
              {"ratingKey": "11", "title": "Old Agent", "guid": "com.plexapp.agents.imdb://tt0000011?lang=en"},
              {"ratingKey": "12", "title": "Bare", "guid": "plex://movie/abc"}]}""",
            ),
        )
        assertTrue(candidates[0].imdb == "tt0000010" && candidates[0].tmdb == "77")
        assertEquals("tt0000011", candidates[1].imdb)
        assertTrue(candidates[2].imdb == null && candidates[2].tmdb == null)
        val leaves = json("""{"Metadata": [{"ratingKey": "22", "parentIndex": 1, "index": 1}, {"ratingKey": "23", "parentIndex": 1, "index": 2}]}""")
        assertEquals("23", Plex.readEpisode(leaves, 1, 2))
        assertNull(Plex.readEpisode(leaves, 2, 2))
    }

    @Test
    fun aShowListsItsEpisodes() {
        val detail = json("""{"Metadata": [{"type": "show", "ratingKey": "20", "title": "A Show"}]}""")
        val leaves = json("""{"Metadata": [{"ratingKey": "22", "title": "Pilot", "parentIndex": 1, "index": 1, "originallyAvailableAt": "2020-02-02"}]}""")
        val meta = Plex.readDetail(server, detail, leaves)!!
        assertEquals("series", meta.kind)
        assertEquals("srv:5:22", meta.videos[0].id)
        assertEquals("2020-02-02", meta.videos[0].released)
    }

    @Test
    fun thePlexCodeLeadsToTheAccountsServers() = runBlocking {
        val web = FakeWeb().apply {
            answer("https://plex.tv/api/v2/pins?strong=true", """{"id": 77, "code": "ABCD"}""")
            answer("https://plex.tv/api/v2/pins/77?code=ABCD", """{"authToken": "acct"}""")
            answer(
                "https://plex.tv/api/v2/resources?includeHttps=1&includeRelay=1",
                """[{"name": "Den", "provides": "server", "accessToken": "srv", "connections": [
               {"uri": "http://den-local.test:32400", "local": true}, {"uri": "https://den-remote.test", "local": false}]}]""",
            )
            // The local address does not answer; the remote one does.
            answer("https://den-remote.test/identity", """{"MediaContainer": {}}""")
        }
        ServerAccounts.plexAskEveryMillis = 10
        var shown: Plex.Pin? = null
        val outcome = ServerAccounts.plexSignIn(ServerClient("dev-1", web.http)) { shown = it }
        assertEquals("ABCD", shown?.code)
        assertTrue(shown!!.link.startsWith("https://app.plex.tv/auth#?clientID=dev-1&code=ABCD"))
        val servers = (outcome as ServerAccounts.PlexOutcome.Servers).servers
        assertEquals(listOf("https://den-remote.test"), servers.map { it.address })
        assertTrue(servers[0].token == "srv" && servers[0].kind == ServerKind.Plex)
    }

    @Test
    fun anAndroidTvGetsAShortCodeForPlexTvLink() = runBlocking {
        val web = FakeWeb().apply { answer("https://plex.tv/api/v2/pins", """{"id": 5, "code": "WXYZ"}""") }
        val pin = Plex.pin(ServerClient("dev-1", web.http, tv = true))
        assertEquals("WXYZ" to "https://plex.tv/link", pin.code to pin.link)
    }
}
