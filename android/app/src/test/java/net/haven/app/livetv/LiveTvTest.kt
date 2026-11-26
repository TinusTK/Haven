package net.haven.app.livetv

import kotlinx.coroutines.runBlocking
import net.haven.app.core.Problem
import net.haven.app.core.Store
import net.haven.app.core.SwitchedOff
import net.haven.app.player.PlayableAddress
import net.haven.app.servers.Wanted
import net.haven.app.testing.FakeWeb
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/** The Rust build's Live TV tests (haven-livetv lib, m3u, xtream) and the
 *  player's address tests (haven-player source), ported. */
class M3UTest {
    private fun channel(name: String, group: String?) = Channel(name, "http://example.org/a", group)

    @Test
    fun groupsKeepTheirFirstOrder() {
        assertEquals(listOf("News", "Sport"), M3U.groups(listOf(channel("A", "News"), channel("B", null), channel("C", "Sport"), channel("D", "News"))))
    }

    @Test
    fun filterTakesAGroupAndWordsInAnyCase() {
        val one = channel("BBC World News HD", "News")
        assertTrue(M3U.matches(one, null, ""))
        assertTrue(M3U.matches(one, "News", "world bbc"))
        assertFalse(M3U.matches(one, "Sport", ""))
        assertFalse(M3U.matches(one, null, "cnn"))
        assertFalse(M3U.matches(channel("X", null), "News", ""))
    }

    @Test
    fun aSourceIsNamedAfterItsHost() {
        assertEquals("tv.example.org", M3U.sourceName("https://tv.example.org/lists/all.m3u"))
        assertEquals("10.0.0.2", M3U.sourceName("http://user:secret@10.0.0.2:8080/get.php?x=1"))
        assertEquals("[::1]", M3U.sourceName("http://[::1]:80/a"))
        assertEquals("nonsense", M3U.sourceName("nonsense"))
    }

    companion object {
        // Made up for these tests.
        val sample = "﻿#EXTM3U url-tvg=\"http://example.org/guide.xml\"\r\n" +
            "#EXTINF:-1 tvg-id=\"one.example\" tvg-logo=\"http://example.org/one.png\" group-title=\"News\",Channel One\r\n" +
            "#EXTVLCOPT:http-user-agent=Example\r\n" +
            "http://example.org/one.m3u8\r\n" +
            "\r\n" +
            "#EXTINF:-1 group-title=\"Sport;Local\" tvg-name=\"Unused\",Two, the \"Second\"\n" +
            "http://example.org/two.ts\n" +
            "#extinf:0 radio=\"true\" Group-Title=\"Music\",Radio Three\n" +
            "#EXTGRP:Ignored because a group is set\n" +
            "http://example.org/three.mp3\n" +
            "#EXTINF:-1,Four\n" +
            "#EXTGRP:Old Style\n" +
            "rtsp://example.org/four\n" +
            "#EXTINF:-1 tvg-name=\"Named By Attribute\",\n" +
            "http://example.org/five\n" +
            "#EXTINF:-1,No address follows\n" +
            "#EXTINF:-1 tvg-logo=\"\",Six\n" +
            "http://example.org/six\n" +
            "http://example.org/path/seven.m3u8?token=1\n"
    }

    @Test
    fun readsChannelsWithTheirDetails() {
        val channels = M3U.parse(sample)
        assertEquals(listOf("Channel One", "Two, the \"Second\"", "Radio Three", "Four", "Named By Attribute", "Six", "seven.m3u8"), channels.map { it.name })
        val one = channels[0]
        assertEquals("http://example.org/one.m3u8", one.address)
        assertEquals("News", one.group)
        assertEquals("http://example.org/one.png", one.logo)
        assertEquals("one.example", one.guideId)
        assertFalse(one.radio)
        assertEquals("Sport", channels[1].group)
        assertTrue(channels[2].radio)
        assertEquals("Music", channels[2].group)
        assertEquals("Old Style", channels[3].group)
        assertEquals("rtsp://example.org/four", channels[3].address)
        assertNull(channels[5].logo)
        assertNull(channels[6].group)
    }

    @Test
    fun textThatIsNotAPlaylistGivesWhatItCan() {
        assertTrue(M3U.parse("").isEmpty())
        assertTrue(M3U.parse("#EXTM3U\n#EXTINF:-1,Alone\n").isEmpty())
        // A web page instead of a playlist: lines are taken as addresses, which the app then refuses.
        assertEquals(1, M3U.parse("<html>").size)
        assertTrue(LiveTV.playable(M3U.parse("<html>")).isEmpty())
    }
}

class XtreamTest {
    private val account get() = XtreamAccount.of("tv.example.org:8080", "me", "p&ss word")!!

    @Test
    fun theTypedServerIsTidied() {
        assertEquals("http://tv.example.org:8080", account.server)
        val pasted = XtreamAccount.of(" HTTPS://tv.example.org/get.php?username=x&password=y&type=m3u_plus ", " me ", " secret ")!!
        assertTrue(pasted.server == "https://tv.example.org" && pasted.username == "me" && pasted.password == "secret")
        for ((server, user, password) in listOf(
            Triple("", "me", "x"), Triple("tv.example.org", "", "x"), Triple("tv.example.org", "me", ""), Triple("ftp://tv.example.org", "me", "x"),
        )) {
            assertNull(server, XtreamAccount.of(server, user, password))
        }
    }

    @Test
    fun anAccountSurvivesBeingStoredAsAnAddress() {
        val url = account.sourceUrl
        assertEquals("http://tv.example.org:8080/player_api.php?username=me&password=p%26ss%20word", url)
        assertEquals(account, XtreamAccount.fromSourceUrl(url))
        assertEquals("$url&action=get_live_streams", account.api("get_live_streams"))
        // A playlist address, or an API address with an action, is not an account.
        assertNull(XtreamAccount.fromSourceUrl("http://tv.example.org/get.php?username=a&password=b"))
        assertNull(XtreamAccount.fromSourceUrl(account.api("x")))
        assertEquals("me at tv.example.org", account.label)
        assertFalse(account.toString().contains("ss word"))
        // A "+" in a stored address is a space.
        assertEquals("b c", XtreamAccount.fromSourceUrl("http://a.example/player_api.php?username=b+c&password=d")?.username)
    }

    // Made up in the shape Xtream Codes servers answer with.
    @Test
    fun signingInReadsTheAccountState() {
        val fine = """{"user_info": {"auth": 1, "status": "Active", "exp_date": "1900000000", "allowed_output_formats": ["m3u8", "ts"]}, "server_info": {}}"""
        assertEquals(XtreamInfo(1_900_000_000, listOf("m3u8", "ts")), Xtream.signIn(fine))
        assertThrows(XtreamError.Refused::class.java) { Xtream.signIn("""{"user_info": {"auth": 0}}""") }
        assertEquals("expired", assertThrows(XtreamError.NotActive::class.java) { Xtream.signIn("""{"user_info": {"auth": "1", "status": "Expired"}}""") }.status)
        assertThrows(XtreamError.BadAnswer::class.java) { Xtream.signIn("<html>") }
        assertThrows(XtreamError.BadAnswer::class.java) { Xtream.signIn("[]") }
        assertEquals(XtreamInfo(), Xtream.signIn("""{"user_info": {"auth": 1, "exp_date": null}}"""))
    }

    @Test
    fun providerNamesAreStrippedDownToTheTitle() {
        for ((name, plain, year) in listOf(
            Triple("EN - Dune: Part Two (2024)", "duneparttwo", "2024"),
            Triple("Dune Part Two 2024 4K", "duneparttwo", "2024"),
            Triple("UK| The Film [Multi-Sub] HD", "thefilm", null),
            Triple("2001: A Space Odyssey (1968)", "2001aspaceodyssey", "1968"),
            Triple("1917", "1917", null),
            Triple("Blade Runner 2049", "bladerunner", "2049"),
        )) {
            assertEquals(name, plain to year, Xtream.plainTitle(name))
        }
    }

    @Test
    fun filmsAndSeriesAreMatchedAndEpisodesFound() {
        val films = Xtream.films(
            """[{"name": "EN - A Film (2021)", "stream_id": 11, "container_extension": "mkv"},
             {"name": "A Film 2021 4K", "stream_id": "12", "container_extension": "mp4", "tmdb": "500"},
             {"name": "A Film (1984)", "stream_id": 13, "container_extension": "avi"},
             {"name": "Another Film", "stream_id": 14, "tmdb": "0", "year": "2021"},
             {"name": "No id"}]""",
        )
        assertEquals(4, films.size)
        // The one with a TMDB number is judged by it alone.
        assertEquals(listOf("11", "12"), Xtream.matches(films, "A Film", "2021", "500").map { it.id })
        assertEquals(listOf("11"), Xtream.matches(films, "A Film", "2021", "999").map { it.id })
        assertEquals(listOf("13"), Xtream.matches(films, "A Film", "1984", null).map { it.id })
        assertTrue(Xtream.matches(films, "", null, null).isEmpty())
        assertThrows(XtreamError.BadAnswer::class.java) { Xtream.films("{}") }

        val series = Xtream.series("""[{"name": "A Show", "series_id": 7, "releaseDate": "2019-04-01"}]""")
        assertTrue(series[0].id == "7" && series[0].year == "2019")

        val info = """{"episodes": {"1": [{"id": "101", "episode_num": 1, "season": 1, "container_extension": "mkv"}, {"id": "102", "episode_num": "2", "season": "1", "container_extension": "mp4"}], "2": [{"id": "201", "episode_num": 1, "season": 2}]}}"""
        assertEquals("102" to "mp4", Xtream.episode(info, 1, 2))
        assertEquals("201" to "", Xtream.episode(info, 2, 1))
        assertNull(Xtream.episode(info, 3, 1))
        assertEquals("9", Xtream.episode("""{"episodes": [[{"id": "9", "episode_num": 4, "season": 3}]]}""", 3, 4)?.first)

        assertEquals("http://tv.example.org:8080/movie/me/p%26ss%20word/12.mp4", account.film("12", "mp4"))
        assertEquals("http://tv.example.org:8080/series/me/p%26ss%20word/201.mp4", account.episode("201", ""))
    }

    @Test
    fun channelsComeWithGroupsAndSurviveThePlaylistForm() {
        val categories = """[{"category_id": "1", "category_name": "News; World"}, {"category_id": "2", "category_name": "Music"}]"""
        val streams = """[{"num": 1, "name": "Channel \"One\"", "stream_type": "live", "stream_id": 101,
              "stream_icon": "http://tv.example.org/one.png", "epg_channel_id": "one.example", "category_id": "1"},
             {"name": "Radio Two", "stream_type": "radio_streams", "stream_id": "202", "category_id": 2},
             {"name": "No category", "stream_id": 303, "category_id": "9", "stream_icon": ""},
             {"name": "No id"}, {"stream_id": 404}]"""
        val info = XtreamInfo()
        val channels = Xtream.channels(account, info, categories, streams)
        assertEquals(3, channels.size)
        assertEquals("http://tv.example.org:8080/live/me/p%26ss%20word/101.ts", channels[0].address)
        assertEquals("News; World", channels[0].group)
        assertEquals("one.example", channels[0].guideId)
        assertTrue(channels[1].radio && !channels[0].radio)
        assertEquals("Music", channels[1].group)
        assertTrue(channels[2].group == null && channels[2].logo == null)

        val readBack = M3U.parse(M3U.write(channels))
        assertEquals(3, readBack.size)
        assertEquals("Channel \"One\"", readBack[0].name)
        assertEquals(channels[0].address, readBack[0].address)
        assertEquals("News, World", readBack[0].group)
        assertEquals(channels[0].logo, readBack[0].logo)
        assertTrue(readBack[1].radio)
        assertEquals(channels[2], readBack[2])

        // No category list still gives the channels.
        assertEquals(3, Xtream.channels(account, info, "oops", streams).size)
        assertThrows(XtreamError.BadAnswer::class.java) { Xtream.channels(account, info, categories, "{}") }
        assertTrue(Xtream.channels(account, XtreamInfo(formats = listOf("m3u8")), categories, streams)[0].address.endsWith("/101.m3u8"))
    }
}

class PlayableAddressTest {
    @Test
    fun webAddressesPass() {
        assertEquals("https://example.org/a.mp4", PlayableAddress.web("https://example.org/a.mp4"))
        assertEquals("HTTP://10.0.0.2:8080/live.m3u8", PlayableAddress.web("  HTTP://10.0.0.2:8080/live.m3u8\n"))
        assertEquals("http://[::1]/v.mkv", PlayableAddress.web("http://[::1]/v.mkv"))
    }

    @Test
    fun televisionAddressesPassAsStreamsOnly() {
        for (address in listOf("rtsp://10.0.0.5/live", "udp://@239.0.0.1:1234", "RTMP://example.org/app/key")) {
            assertEquals(address, PlayableAddress.stream(address))
            assertNull(PlayableAddress.web(address))
        }
        assertEquals("https://example.org/a.m3u8", PlayableAddress.stream("https://example.org/a.m3u8"))
        for (address in listOf("file:///etc/passwd", "/dev/video0", "edl://x", "<html>", "https://@")) assertNull(address, PlayableAddress.stream(address))
    }

    @Test
    fun everythingElseIsRefused() {
        for (address in listOf(
            "", "/etc/passwd", "file:///etc/passwd", "edl://a.mkv", "lavfi://testsrc", "av://v4l2:/dev/video0",
            "smb://server/share/a.mkv", "magnet:?xt=urn:btih:abc", "https://", "https:///no-host",
            "https://example.org/a b.mp4", "https://example.org/a\nb.mp4", "https://@example.org/a.mp4",
            "javascript://https://example.org",
        )) {
            assertNull(address, PlayableAddress.web(address))
        }
    }
}

/** What the Live TV screens show, and adding sources. */
class LiveTvTest {
    private val channels = listOf(
        Channel("BBC One", "http://a/1", "News"),
        Channel("Sky Sports", "http://a/2", "Sport"),
        Channel("BBC News", "http://a/3", "News"),
    )

    @Test
    fun categoriesListFavouritesThenAllThenGroups() {
        val favourites = listOf(LiveTV.FavouriteGroup("Mine", listOf("http://a/2")), LiveTV.FavouriteGroup("Empty"))
        val rows = LiveTV.categories(channels, radio = false, favourites = favourites, words = "")
        assertEquals(listOf("f:Mine", "f:Empty", "", "g:News", "g:Sport"), rows.map { it.key })
        assertEquals(listOf(1, 0, 3, 2, 1), rows.map { it.count })
        assertEquals("All channels", rows[2].label)
        // Radio lists only groups that hold stations; the filter leaves favourites alone.
        val radio = LiveTV.categories(channels, radio = true, favourites = favourites, words = "spo")
        assertEquals(listOf("f:Mine", "", "g:Sport"), radio.map { it.key })
        assertEquals("All stations", radio[1].label)
        assertTrue(LiveTV.categoryExists("g:News", channels, favourites))
        assertFalse(LiveTV.categoryExists("g:Film", channels, favourites))
        assertTrue(LiveTV.categoryExists("f:Empty", channels, favourites))
    }

    @Test
    fun channelsAreFilteredAndStarred() {
        val favourites = listOf(LiveTV.FavouriteGroup("Mine", listOf("http://a/3")))
        val news = LiveTV.channelRows(channels, "g:News", "bbc", favourites)
        assertEquals(listOf(0, 2), news.map { it.index })
        assertEquals(listOf(false, true), news.map { it.starred })
        assertEquals(listOf("BBC News"), LiveTV.channelRows(channels, "f:Mine", "", favourites).map { it.name })
        assertEquals(listOf("BBC One"), LiveTV.channelRows(channels, "", "one bbc", favourites).map { it.name })
    }

    @Test
    fun favouriteGroupsAreMadeFilledAndDeleted() {
        var favourites = emptyList<LiveTV.FavouriteGroup>()
        favourites = LiveTV.newGroup(" Sport ", "http://a/2", favourites)
        favourites = LiveTV.newGroup("Sport", "http://a/2", favourites)
        favourites = LiveTV.newGroup("  ", null, favourites)
        assertEquals(listOf(LiveTV.FavouriteGroup("Sport", listOf("http://a/2"))), favourites)
        favourites = LiveTV.toggle("http://a/1", "Sport", favourites)
        favourites = LiveTV.toggle("http://a/2", "Sport", favourites)
        assertEquals(listOf("http://a/1"), favourites[0].channels)
        val store = Store.inMemory()
        LiveTV.save(favourites, store)
        assertEquals(favourites, LiveTV.favourites(store))
        assertEquals(1, store.get<List<LiveTV.FavouriteGroup>>(LiveTV.FAVOURITES_KEY)?.size)
        assertTrue(LiveTV.deleteGroup("Sport", favourites).isEmpty())
    }

    @Test
    fun addingAPlaylistKeepsItAndSaysWhatCame() {
        val store = Store.inMemory()
        val url = "http://lists.example/all.m3u"
        val fetched = LiveTV.fetched(url, null, adding = true, result = Result.success(M3UTest.sample), store = store)
        assertTrue(fetched.saved)
        assertEquals(LiveTV.Status("Added 6 channels and 1 radio station. They are on the Live TV tab.", false, false), fetched.status)
        val (tv, radio) = LiveTV.boards(store)
        assertTrue(tv.size == 6 && radio.size == 1)
        assertEquals(listOf("lists.example"), LiveTV.sourceRows(store).map { it.name })

        // An empty list keeps what was there; a failed refresh says nothing.
        val empty = LiveTV.fetched(url, null, adding = false, result = Result.success("#EXTM3U"), store = store)
        assertTrue(!empty.saved && empty.status == null)
        assertEquals(6, LiveTV.boards(store).first.size)
        assertEquals("No channels were found at that address.", LiveTV.fetched(url, null, true, Result.success("<html>"), store).status?.text)
        assertEquals(
            "Could not add the source: the server answered with error 404.",
            LiveTV.fetched(url, null, true, Result.failure(Problem("the server answered with error 404")), store).status?.text,
        )

        // Switched off: listed, its channels not shown. Removed: gone, and a refresh on the way does not bring it back.
        assertNull(LiveTV.setEnabled(url, on = false, store = store))
        assertTrue(LiveTV.boards(store).first.isEmpty() && !LiveTV.sourceRows(store)[0].enabled)
        assertEquals("Source removed.", LiveTV.remove(url, store).text)
        assertTrue(SwitchedOff.load(store).tvSources.isEmpty())
        assertFalse(LiveTV.fetched(url, null, adding = false, result = Result.success(M3UTest.sample), store = store).saved)
        assertTrue(LiveTV.sourceRows(store).isEmpty())
    }

    @Test
    fun anAccountIsListedWithoutItsPassword() {
        val store = Store.inMemory()
        val account = XtreamAccount.of("tv.example.org", "me", "secret")!!
        val playlist = M3U.write(listOf(Channel("One", account.stream("1", "ts"))))
        val fetched = LiveTV.fetched(account.sourceUrl, account, true, Result.success(playlist), store)
        assertTrue(fetched.online == true && fetched.status?.text == "Added 1 channel. They are on the Live TV tab.")
        assertTrue(fetched.status!!.account)
        val row = LiveTV.sourceRows(store).first()
        assertTrue(row.name == "me at tv.example.org" && row.detail == "Xtream Codes account of me" && row.isAccount)
        assertEquals(listOf(account), LiveTV.accounts(store))
        assertEquals(false, LiveTV.fetched(account.sourceUrl, account, false, Result.failure(Problem("x")), store).online)
        assertEquals(
            LiveTV.Status("Could not add the source: \"not an address\" is not a web address.", true, false),
            LiveTV.playlistAddress("not an address").second,
        )
    }

    @Test
    fun anAccountIsFetchedThroughItsApi() = runBlocking {
        val account = XtreamAccount.of("xt1.test", "u", "p")!!
        val refused = XtreamAccount.of("xt2.test", "u", "bad")!!
        val web = FakeWeb().apply {
            answer(account.api(), """{"user_info": {"auth": 1, "status": "Active"}}""")
            answer(account.api("get_live_streams"), """[{"name": "One", "stream_id": 5, "category_id": "1"}]""")
            answer(refused.api(), """{"user_info": {"auth": 0}}""")
        }
        val text = LiveTV.fetch(account.sourceUrl, web.http).getOrThrow()
        assertEquals(listOf("http://xt1.test/live/u/p/5.ts"), M3U.parse(text).map { it.address })
        assertTrue(LiveTV.check(account, web.http))
        assertEquals("the user name or password was not accepted", LiveTV.fetch(refused.sourceUrl, web.http).exceptionOrNull()?.message)
        assertFalse(LiveTV.check(refused, web.http))
    }

    @Test
    fun filmsAndEpisodesComeFromAnAccountsLists() = runBlocking {
        val account = XtreamAccount.of("xt3.test", "u", "p")!!
        val web = FakeWeb().apply {
            answer(account.api("get_vod_streams"), """[{"name": "EN - A Film (2021)", "stream_id": 11, "container_extension": "mkv"}]""")
            answer(account.api("get_series"), """[{"name": "A Show", "series_id": 7}]""")
            answer("${account.api("get_series_info")}&series_id=7", """{"episodes": {"1": [{"id": "101", "episode_num": 2, "season": 1}]}}""")
        }
        val lists = XtreamOnDemand(web.http)
        val films = lists.streams(account, Wanted(name = "A Film", year = "2021")).getOrThrow()
        assertEquals(listOf("http://xt3.test/movie/u/p/11.mkv"), films.map { it.url })
        assertTrue(films[0].description == "MKV" && films[0].name == "EN - A Film (2021)")
        val episode = lists.streams(account, Wanted(series = true, name = "A Show", episode = 1 to 2)).getOrThrow()
        assertEquals(listOf("A Show  S1 E2"), episode.map { it.name })
        assertTrue(episode[0].url == "http://xt3.test/series/u/p/101.mp4" && episode[0].description == "File")
        assertFalse(XtreamOnDemand.worthAsking(Wanted(series = true, name = "A Show")))
        assertEquals("Xtream · u at xt3.test", XtreamOnDemand.sourceLabel(account))
        val broken = XtreamAccount.of("xt4.test", "u", "p")!!
        assertEquals("the server answered with error 404", lists.streams(broken, Wanted(name = "A Film")).exceptionOrNull()?.message)
    }
}
