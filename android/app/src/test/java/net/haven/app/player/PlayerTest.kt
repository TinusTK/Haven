package net.haven.app.player

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import net.haven.app.core.Secrets
import net.haven.app.core.Store
import net.haven.app.library.Library
import net.haven.app.library.Opening
import net.haven.app.library.PlayRequest
import net.haven.app.pictures.Kept
import net.haven.app.testing.FakeWeb
import net.haven.app.titles.Cinemeta
import net.haven.app.titles.ResourceUrl
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.time.Instant
import java.time.ZoneOffset
import java.util.concurrent.Executors

/** The Rust build's player tests (`haven-player/src/tracks.rs`,
 *  `haven-app/src/player.rs`, `external.rs`), ported. */
class PlayerRulesTest {
    private val sample = """
        [
            {"id": 1, "type": "video", "selected": true, "codec": "h264", "demux-w": 1920},
            {"id": 1, "type": "audio", "lang": "eng", "title": "English", "selected": true, "codec": "ac3", "demux-channel-count": 6},
            {"id": 2, "type": "audio", "lang": "eng", "title": "Commentary", "selected": false, "codec": "aac", "demux-channel-count": 2},
            {"id": 3, "type": "audio", "selected": false},
            {"id": 1, "type": "sub", "lang": "nl", "selected": false, "codec": "subrip"},
            {"id": 2, "type": "sub", "lang": "en", "title": "Signs", "forced": true, "selected": true},
            {"id": 3, "type": "sub", "lang": "und", "title": "  ", "selected": false},
            {"type": "sub"},
            "not a track"
        ]
    """

    @Test fun namesAudioAndSubtitleTracks() {
        val seen = PlayerRules.tracks(sample).map { "${if (it.kind == Track.Kind.Audio) "a" else "s"}${it.id} ${it.label} ${it.selected}" }
        assertEquals(
            listOf("a1 English · AC3 5.1 true", "a2 English · Commentary · AAC Stereo false", "a3 Track 3 false", "s1 Dutch false", "s2 English · Signs · Forced true", "s3 Track 3 false"),
            seen,
        )
    }

    @Test fun anythingThatIsNotATrackListGivesNoTracks() {
        assertTrue(PlayerRules.tracks("").isEmpty())
        assertTrue(PlayerRules.tracks("{}").isEmpty())
        assertTrue(PlayerRules.tracks("[]").isEmpty())
    }

    @Test fun unknownLanguageCodesAreShownAsTheyAre() {
        assertEquals("Portuguese", PlayerRules.languageName("pt-BR"))
        assertEquals("English", PlayerRules.languageName("ENG"))
        assertEquals("TLH", PlayerRules.languageName("tlh"))
    }

    @Test fun aStreamsArrivalIsPutInWords() {
        assertEquals("4.2 MB/s", PlayerRules.arrival(4_200_000.0, null))
        assertEquals("850 KB/s  ·  42% loaded", PlayerRules.arrival(850_000.0, 42.7))
        assertEquals("100% loaded", PlayerRules.arrival(null, 99.8))
        assertEquals("", PlayerRules.arrival(0.0, null))
        assertEquals("", PlayerRules.arrival(null, null))
        assertEquals(20.0, PlayerRules.shareLoaded(PlayerEngine.Status(600.0, 6000.0, false, false, null, 600.0))!!, 0.0)
        assertNull(PlayerRules.shareLoaded(PlayerEngine.Status(5.0, 0.0, false, false)))
    }

    @Test fun playerAppsAreHandedOnlyWebAddresses() {
        val other = PlayerRules.OtherPlayer(kind = 1)
        assertTrue(PlayerRules.goesElsewhere(other, "https://example.com/film.mkv?x=1&y=2"))
        assertFalse(PlayerRules.goesElsewhere(other, "/data/user/0/net.haven.app/files/film.mkv"))
        assertFalse(PlayerRules.goesElsewhere(other, "file:///sdcard/film.mkv"))
        assertFalse(PlayerRules.goesElsewhere(PlayerRules.OtherPlayer(), "https://example.com/film.mkv"))
    }

    @Test fun theSettingsReadAsTheRustBuildWroteThem() {
        val json = Json { ignoreUnknownKeys = true }
        val read = json.decodeFromString<PlayerRules.Preferences>("""{"volume": 0.4, "muted": true, "subtitle_scale": 1.3, "passthrough": true}""")
        assertTrue(read.volume == 0.4 && read.muted && read.subtitleScale == 1.3 && read.passthrough)
        assertEquals(PlayerRules.Preferences(), json.decodeFromString<PlayerRules.Preferences>("{}"))
        assertTrue(Json.encodeToString(PlayerRules.Preferences.serializer(), read).contains("\"subtitle_scale\""))
        val other = json.decodeFromString<PlayerRules.OtherPlayer>("""{"kind": 2}""")
        assertTrue(other.kind == 2 && other.program.isEmpty())
        assertEquals(1.0, PlayerRules.Preferences().top(tv = false), 0.0)
        assertEquals(2.0, PlayerRules.Preferences(loud = true).top(tv = false), 0.0)
        assertEquals("a TV has the Sound boost", 2.0, PlayerRules.Preferences().top(tv = true), 0.0)
    }

    @Test fun theControlsSayWhatIsKnown() {
        val now = Instant.ofEpochSecond(20 * 3600)
        assertEquals("Ends at 21:30", PlayerRules.endsAt(5400.0, 1.0, now, ZoneOffset.UTC))
        assertEquals("Ends at 20:45", PlayerRules.endsAt(5400.0, 2.0, now, ZoneOffset.UTC))
        val details = PlayerEngine.Details(1920, 1080, 23.976, "h264", "eac3", "5.1", "mkv", "mediacodec-copy")
        assertEquals(
            "Picture: 1920 × 1080, 23.976 frames a second\nVideo: h264 (decoded by mediacodec-copy)\nSound: eac3, 5.1\nFile: mkv\nFrom: Home (Jellyfin)",
            PlayerRules.info(details, "Home (Jellyfin)"),
        )
        assertEquals("", PlayerRules.info(PlayerEngine.Details(), ""))
        assertTrue(PlayerRules.speedLabel(1.0) == "Normal" && PlayerRules.speedLabel(1.5) == "1.5×" && PlayerRules.speedLabel(2.0) == "2×")
    }

    @Test fun mpvsReasonsAreInWords() {
        assertEquals("the address could not be opened", PlayerEngine.reason(-13))
        assertEquals("the format was not recognised", PlayerEngine.reason(-17))
        assertEquals("there was no picture or sound in it", PlayerEngine.reason(-16))
        assertEquals("mpv error -99", PlayerEngine.reason(-99))
    }
}

/** mpv's stand-in: plays nothing, remembers what it was told. */
class FakeEngine : PlayerEngine {
    var played: Pair<String, Double?>? = null
    var stops = 0
    var volumeSet = 1.0
    var speedSet = 1.0
    val events = mutableListOf<PlayerEngine.Event>()
    var now: PlayerEngine.Status? = null

    override val broken: String? = null
    override val status get() = now
    override val tracksJson = ""
    override val details = PlayerEngine.Details()
    override fun play(address: String, at: Double?) { played = address to at }
    override fun stop() { stops += 1 }
    override fun poll() = events.toList().also { events.clear() }
    override fun setPaused(paused: Boolean) { now = now?.copy(paused = paused) }
    override fun seekTo(seconds: Double) {}
    override fun seekBy(seconds: Double) {}
    override fun selectAudio(id: Int) {}
    override fun selectSubtitle(id: Int?) {}
    override fun setSubtitleMargin(margin: Int) {}
    override fun setVolume(volume: Double) { volumeSet = volume }
    override fun setMuted(muted: Boolean) {}
    override fun setPassthrough(on: Boolean) {}
    override fun setSpeed(speed: Double) { speedSet = speed }
    override fun setFit(fit: PlayerEngine.Fit) {}
    override fun setSubtitleScale(scale: Double) {}
    override fun setSubtitleDelay(seconds: Double) {}
    override fun syncSubtitles(way: Int): Double? = null
}

class PlayerModelTest {
    private val thread = Executors.newSingleThreadExecutor().asCoroutineDispatcher()
    private val scopes = mutableListOf<CoroutineScope>()

    @After fun stop() = scopes.forEach { it.cancel() }

    private fun scope() = CoroutineScope(SupervisorJob() + thread).also { scopes += it }
    private fun store() = Store.open(File.createTempFile("haven-test-", ".sqlite3").also { it.delete(); it.deleteOnExit() })
    private fun <T> on(block: () -> T): T = runBlocking(thread) { block() }

    private fun request(video: String = "tt7700003", next: String? = null) =
        PlayRequest("https://x/f.mp4", "A Film", "", "movie", "tt7700003", video, null, "Somewhere", next = next, serverItem = "srv:4:abc")

    @Test fun onlyAPlacePartwayOrAtTheEndIsKept() {
        val store = store()
        PlayerModel.remember(request(), 10.0, 6000.0, store)
        assertNull("barely started", store.progress("movie", "tt7700003", "tt7700003"))
        PlayerModel.remember(request(), 600.0, 6000.0, store)
        val partway = store.progress("movie", "tt7700003", "tt7700003")!!
        assertTrue(partway.position == 600.0 && !partway.finished && partway.title == "A Film")
        PlayerModel.remember(request(), 5800.0, 6000.0, store)
        assertEquals(true, store.progress("movie", "tt7700003", "tt7700003")?.finished)
        assertNull(PlayerModel.played(request().copy(remember = false), 600.0, 6000.0, false))
        assertNull(PlayerModel.played(request(), 600.0, 6000.0, true))
        assertNull("a live stream", PlayerModel.played(request(), 600.0, 0.0, false))
        val played = PlayerModel.played(request(), 5800.0, 6000.0, false)!!
        assertTrue(played.finished && played.serverItem == "srv:4:abc")
    }

    @Test fun playingResumesWhereItWasLeft() {
        val store = store()
        PlayerModel.remember(request(), 600.0, 6000.0, store)
        val engine = FakeEngine()
        val player = PlayerModel(store, engine, scope())
        on { player.play(request(next = "S1 E2 · Two")) }
        assertTrue("3 s back", player.open && player.busy && player.position == 597.0)
        assertEquals("https://x/f.mp4" to 597.0, engine.played)
        assertTrue(player.nextTitle == "S1 E2 · Two" && player.title == "A Film")
        val closed = mutableListOf<Played?>()
        player.closed = { closed += it }
        on { player.close() }
        assertTrue(!player.open && closed.size == 1)
        assertNull("it never loaded: its length is unknown, so nothing is recorded", closed.first())
        assertEquals("the place stays", 600.0, store.progress("movie", "tt7700003", "tt7700003")?.position)
    }

    @Test fun anEpisodeAtItsEndCountsDownToTheNext() {
        val store = store()
        val engine = FakeEngine()
        val player = PlayerModel(store, engine, scope())
        var asked: Played? = null
        player.next = { asked = it; true }
        on {
            player.play(request(next = "S1 E2 · Two"))
            engine.events += PlayerEngine.Event.Loaded
            engine.now = PlayerEngine.Status(5990.0, 6000.0, false, false)
            player.tick()
            engine.events += PlayerEngine.Event.Finished
            player.tick()
        }
        assertEquals(10, player.nextCountdown)
        assertEquals(true, store.progress("movie", "tt7700003", "tt7700003")?.finished)
        on { player.goNext() }
        assertTrue(asked?.finished == true && player.subtitle == "S1 E2 · Two" && player.nextCountdown == null)
    }

    @Test fun theSettingsAreKept() {
        val store = store()
        val player = PlayerModel(store, FakeEngine(), scope())
        on {
            player.setVolume(0.3)
            player.toggleMute()
            player.stepSubtitleScale(1)
            player.stepSubtitleScale(1)
        }
        val engine = FakeEngine()
        val again = PlayerModel(store, engine, scope())
        assertTrue(again.preferences.volume == 0.3 && again.preferences.muted)
        assertEquals(1.2, again.preferences.subtitleScale, 0.0)
        on {
            // Moving the volume turns the sound back on; above 100% only when loud.
            again.setVolume(1.5)
            assertTrue(!again.preferences.muted && again.preferences.volume == 1.0)
            again.setLoud(true)
            again.setVolume(1.5)
            assertEquals(1.5, again.preferences.volume, 0.0)
            assertEquals(1.5, engine.volumeSet, 0.0)
            again.setLoud(false)
            assertEquals("turned off, it comes back to 100%", 1.0, again.preferences.volume, 0.0)
            repeat(30) { again.stepSubtitleScale(-1) }
            assertEquals(0.5, again.preferences.subtitleScale, 0.0)
            again.stepSubtitleDelay(1)
            again.stepSubtitleDelay(1)
            assertEquals(1.0, again.subtitleDelay, 0.0)
            again.chooseSpeed(1.5)
            again.chooseFit(PlayerEngine.Fit.Fill)
        }
        assertTrue(again.speed == 1.5 && again.fit == PlayerEngine.Fit.Fill && engine.speedSet == 1.5)
    }

    @Test fun aTvsSoundBoostGoesTo200() {
        val player = PlayerModel(store(), FakeEngine(), scope(), tv = true)
        on { player.setVolume(2.0) }
        assertEquals(2.0, player.preferences.volume, 0.0)
    }

    private fun library(web: FakeWeb) = Library(store(), Secrets(), web.http, Kept(Files.createTempDirectory("haven-kept").toFile()), scope())

    @Test fun afterPlayingTheServerAndAccountsAreTold() {
        val library = library(FakeWeb())
        val queued = mutableListOf<String>()
        library.queueSync = { what, _, _, _ -> queued += what }
        on {
            library.playerClosed(Played("movie", "tt1", "tt1", 5800.0, 6000.0, true))
            library.playerClosed(Played("movie", "tt1", "tt1", 600.0, 6000.0, false))
            library.playerClosed(null)
        }
        assertEquals(listOf("watched"), queued)
        assertEquals(3, library.marks)
        // Nothing open: no next episode.
        assertFalse(on { library.playNext(null) })
    }

    @Test fun theNextEpisodeOpensAndGivesWayWhenNothingCanPlayIt() {
        val web = FakeWeb().apply {
            answer(ResourceUrl.make(Cinemeta.URL, "meta", "series", "tt7700004"), """
                {"meta": {"id": "tt7700004", "type": "series", "name": "B Show",
                  "videos": [{"id": "tt7700004:1:1", "title": "One", "season": 1, "episode": 1},
                             {"id": "tt7700004:2:1", "title": "Two", "season": 2, "episode": 1}]}}
            """)
        }
        val library = library(web)
        var dismissed = 0
        library.dismissPlayer = { dismissed += 1 }
        val page = library.title
        on { page.open(Opening("tt7700004", "series", "B Show")) }
        repeat(100) { if (on { page.loading }) Thread.sleep(30) }
        on { page.openEpisode("tt7700004:1:1") }
        assertEquals("tt7700004:2:1", page.nextVideo?.id)
        assertFalse("nothing played yet: no place to play from", on { library.playNext(null) })
        library.lastPlayed = "Home (Jellyfin)" to "1080p"
        // Nobody to ask: the player gives way to the list straight away.
        assertFalse(on { library.playNext(null) })
        assertEquals(1, dismissed)
        assertTrue(page.season == 2 && page.episodeLabel == "S2 E1 · Two")
        assertNull(page.sources.autoplay)
    }
}
