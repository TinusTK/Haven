package net.haven.app.cast

import net.haven.app.cast.CastRules.IdleReason
import net.haven.app.cast.CastRules.PlayerState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class CastRulesTest {
    @Test fun contentTypesFollowTheAddress() {
        assertEquals("application/x-mpegURL", CastRules.contentType("https://x/a/master.m3u8?token=1"))
        assertEquals("video/x-matroska", CastRules.contentType("https://x/film.MKV"))
        assertEquals("video/webm", CastRules.contentType("https://x/film.webm#t=3"))
        assertEquals("video/mp4", CastRules.contentType("https://x/Videos/abc/stream?static=true"))
        assertEquals("audio/mpeg", CastRules.contentType("https://x/radio.mp3"))
        assertEquals("application/dash+xml", CastRules.contentType("https://x/a.mpd"))
        assertEquals("audio/mp4", CastRules.contentType("https://x/a.m4a"))
    }

    @Test fun onlyAddressesAChromecastCanReach() {
        assertNull(CastRules.whyNot("https://server.example/Videos/1/stream.mp4?api_key=x"))
        assertNull(CastRules.whyNot("http://192.168.1.20:8096/Videos/1/stream"))
        assertNotNull(CastRules.whyNot("file:///data/user/0/net.haven.app/files/downloads/a.mkv"))
        assertNotNull(CastRules.whyNot("content://media/external/video/1"))
        assertTrue(CastRules.whyNot("http://127.0.0.1:11470/abc/0")!!.contains("this device"))
        assertNotNull(CastRules.whyNot("http://localhost:8080/a.mp4"))
        assertNotNull(CastRules.whyNot("http://[::1]:8080/a.mp4"))
        assertNotNull(CastRules.whyNot("not an address at all"))
    }

    @Test fun startsWhereTheTitleWasLeft() {
        assertEquals(597.0, CastRules.startAt(600.0, 3600.0, false), 0.0)
        assertEquals(0.0, CastRules.startAt(600.0, 3600.0, true), 0.0)
        assertEquals(0.0, CastRules.startAt(10.0, 3600.0, false), 0.0)
        assertEquals(0.0, CastRules.startAt(null, null, false), 0.0)
    }

    @Test fun theCastingBar() {
        assertEquals("Dune", CastRules.titleLine("Dune", ""))
        assertEquals("Severance · S2 E3", CastRules.titleLine("Severance", "S2 E3"))
        assertEquals("4:05 / 1:00:00", CastRules.timeText(245.0, 3600.0))
        assertEquals("4:05", CastRules.timeText(245.0, 0.0))
    }

    @Test fun skipsStayInsideTheVideo() {
        assertEquals(0.0, CastRules.skipTarget(5.0, 100.0, -10.0), 0.0)
        assertEquals(100.0, CastRules.skipTarget(90.0, 100.0, 30.0), 0.0)
        assertEquals(130.0, CastRules.skipTarget(100.0, 0.0, 30.0), 0.0)
        assertEquals(50.0, CastRules.skipTarget(60.0, 100.0, -10.0), 0.0)
    }

    @Test fun knowsWhenPlaybackIsOver() {
        assertNull(CastRules.ending(PlayerState.Playing, IdleReason.None))
        assertNull(CastRules.ending(PlayerState.Buffering, IdleReason.Finished))
        // Idle before anything was loaded is not an end.
        assertNull(CastRules.ending(PlayerState.Idle, IdleReason.None))
        assertEquals(CastEvent.Ended, CastRules.ending(PlayerState.Idle, IdleReason.Finished))
        assertEquals(CastEvent.Ended, CastRules.ending(PlayerState.Idle, IdleReason.Cancelled))
        assertEquals(CastEvent.Ended, CastRules.ending(PlayerState.Idle, IdleReason.Interrupted))
        assertTrue(CastRules.ending(PlayerState.Idle, IdleReason.Error) is CastEvent.Failed)
    }
}
