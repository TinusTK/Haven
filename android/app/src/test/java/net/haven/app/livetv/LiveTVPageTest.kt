package net.haven.app.livetv

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.runBlocking
import net.haven.app.core.Store
import net.haven.app.core.TvSource
import net.haven.app.library.PlayRequest
import net.haven.app.player.FakeEngine
import net.haven.app.player.PlayerModel
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.util.concurrent.Executors

/** The Live TV page's model, and radio going on in the background. */
class LiveTVPageTest {
    private val thread = Executors.newSingleThreadExecutor().asCoroutineDispatcher()
    private val scopes = mutableListOf<CoroutineScope>()

    @After fun stop() = scopes.forEach { it.cancel() }

    private fun scope() = CoroutineScope(SupervisorJob() + thread).also { scopes += it }
    private fun <T> on(block: () -> T): T = runBlocking(thread) { block() }

    private val playlist = """
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
    """.trimIndent()

    private fun store() = Store.open(File.createTempFile("haven-test-", ".sqlite3").also { it.delete(); it.deleteOnExit() })

    private fun model(): Pair<LiveTVModel, Store> {
        val store = store()
        store.saveTvSource(TvSource("https://tv.example/list.m3u", "tv.example", playlist))
        val live = LiveTVModel(store, scope())
        on { live.rebuild() }
        return live to store
    }

    @Test fun channelsAndStationsAreApartAndFiltered() {
        val (live, _) = model()
        assertEquals("a file is never played", listOf("One News", "Two Sport", "Three Sport"), live.tv.channels.map { it.name })
        assertEquals(listOf("All channels", "News", "Sport"), live.categories.map { it.label })
        assertEquals(listOf(3, 1, 2), live.categories.map { it.count })
        assertTrue(live.countText == "3 in all" && live.categoryName == "All channels")
        on { live.choose("g:Sport") }
        assertEquals(listOf("Two Sport", "Three Sport"), live.channels.map { it.name })
        assertTrue(live.countText == "2 of 3" && live.categoryName == "Sport")
        on { live.setWords("three") }
        assertEquals(listOf("Three Sport"), live.channels.map { it.name })
        on { live.setCategoryWords("new") }
        assertEquals(listOf("All channels", "News"), live.categories.map { it.label })
        on { live.chooseBand(1) }
        assertTrue(live.isRadio && live.channels.map { it.name } == listOf("Jazz FM"))
        assertEquals("All stations", live.categories.first().label)
    }

    @Test fun favouritesComeFirstAndAreKept() {
        val (live, store) = model()
        on { live.chooseFavourites(1) }
        assertEquals("Two Sport", live.choosing?.name)
        on { live.newGroup("  Mine ") }
        assertTrue(live.favourites.map { it.name } == listOf("Mine") && live.has(live.favourites[0]))
        on { live.closeChooser() }
        assertTrue(live.categories.first().label == "Mine" && live.categories.first().favourite)
        assertEquals(true, live.channels.firstOrNull { it.name == "Two Sport" }?.starred)
        on { live.choose("f:Mine") }
        assertEquals(listOf("Two Sport"), live.channels.map { it.name })
        // Radio lists only the groups that hold stations.
        on { live.chooseBand(1) }
        assertEquals(listOf("All stations", "Music"), live.categories.map { it.label })
        on { live.chooseBand(0) }
        // Kept for the next start, as the Rust build kept them.
        assertEquals(listOf(listOf("https://tv.example/two.m3u8")), LiveTV.favourites(store).map { it.channels })
        on {
            live.chooseFavourites(1)
            live.toggle("Mine")
        }
        assertTrue("the group is empty now", live.channels.isEmpty())
        on { live.deleteGroup("Mine") }
        assertEquals("a chosen group that is gone falls back to every channel", LiveTV.ALL, live.board.category)
    }

    @Test fun aSwitchedOffSourceIsNotShown() {
        val (live, store) = model()
        LiveTV.setEnabled("https://tv.example/list.m3u", false, store)
        on { live.rebuild() }
        assertTrue(live.tv.channels.isEmpty() && live.radio.channels.isEmpty())
    }

    @Test fun aChannelPlaysWithNoPlaceKept() {
        val (live, _) = model()
        val played = mutableListOf<PlayRequest>()
        live.play = { played += it }
        on {
            live.chooseBand(1)
            live.play(0)
        }
        assertTrue(played.first().kind == "radio" && played.first().title == "Jazz FM" && played.first().detail == "Music")
        assertFalse(played.first().remember)
        on { live.play(9) }
        assertEquals(1, played.size)
    }

    @Test fun radioGoesOnWhenHavenIsLeftAndFilmsWait() {
        val engine = FakeEngine()
        val player = PlayerModel(store(), engine, scope())
        on {
            player.play(PlayRequest("https://radio.example/jazz.mp3", "Jazz FM", "Music", "radio", "", "", null, "", remember = false))
            engine.now = net.haven.app.player.PlayerEngine.Status(5.0, 0.0, false, false)
        }
        assertTrue(player.isRadio)
        on { player.leaving() }
        assertFalse(player.paused)
        on {
            player.close()
            player.play(PlayRequest("https://x/f.mp4", "A Film", "", "movie", "tt1", "tt1", null, ""))
            player.leaving()
        }
        assertTrue(player.paused)
        on { player.close() }
    }
}
