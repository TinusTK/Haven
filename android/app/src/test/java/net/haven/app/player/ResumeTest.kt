package net.haven.app.player

import net.haven.app.player.Resume.Watched
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** As the Rust build's `haven-player::resume` tests. */
class ResumeTest {
    @Test fun judgesTheStartMiddleAndEnd() {
        assertEquals(Watched.Started, Resume.judge(5.0, 3600.0))
        assertEquals(Watched.Partway, Resume.judge(30.0, 3600.0))
        assertEquals(Watched.Partway, Resume.judge(1800.0, 3600.0))
        assertEquals(Watched.Finished, Resume.judge(3420.0, 3600.0))
        assertEquals(Watched.Finished, Resume.judge(3600.0, 3600.0))
    }

    @Test fun skippingTheCreditsStillCountsAsWatched() {
        assertEquals(Watched.Finished, Resume.judge(7200.0 - 540.0, 7200.0))
        assertEquals(Watched.Partway, Resume.judge(7200.0 - 660.0, 7200.0))
        assertEquals(Watched.Finished, Resume.judge(2700.0 - 260.0, 2700.0))
        assertEquals(Watched.Partway, Resume.judge(2700.0 - 300.0, 2700.0))
        assertEquals(Watched.Partway, Resume.judge(1320.0 - 600.0, 1320.0))
    }

    @Test fun aLiveStreamHasNoPlaceToRemember() {
        assertNull(Resume.judge(500.0, 0.0))
        assertNull(Resume.judge(500.0, Double.NaN))
        assertNull(Resume.judge(Double.POSITIVE_INFINITY, 100.0))
        assertNull(Resume.resumeFrom(500.0, 0.0, false))
    }

    @Test fun resumesALittleBeforeWhereItStopped() {
        assertEquals(597.0, Resume.resumeFrom(600.0, 3600.0, false)!!, 0.0)
        assertNull(Resume.resumeFrom(10.0, 3600.0, false))
        assertNull(Resume.resumeFrom(3500.0, 3600.0, false))
        assertNull(Resume.resumeFrom(600.0, 3600.0, true))
    }

    @Test fun shareStaysBetweenNothingAndAll() {
        assertEquals(0.25f, Resume.share(900.0, 3600.0))
        assertEquals(1f, Resume.share(4000.0, 3600.0))
        assertEquals(0f, Resume.share(-1.0, 3600.0))
        assertEquals(0f, Resume.share(10.0, 0.0))
    }

    @Test fun clockShowsHoursOnlyWhenNeeded() {
        assertEquals("0:00", Resume.clock(0.0))
        assertEquals("4:05", Resume.clock(245.9))
        assertEquals("1:02:03", Resume.clock(3723.0))
        assertEquals("0:00", Resume.clock(-5.0))
        assertEquals("0:00", Resume.clock(Double.NaN))
    }
}
