package net.haven.app.library

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Instant
import java.time.ZoneOffset

/** The Calendar page (the Rust build's `show_calendar` and `step_calendar`). */
class CalendarPageTest {
    private val show = Releases.Title("series", "tt1")
    private val film = Releases.Title("movie", "tt2")
    private val gone = Releases.Title("movie", "tt3")
    private val today = Day(2026, 11, 25)

    private fun release(day: Int, time: Clock?, label: String, video: String, month: Int = 11) =
        Release(Day(2026, month, day), time, label, video, "", "")

    private fun page(
        week: Boolean = false,
        anchor: Day = today,
        watched: Map<Releases.Title, Set<String>> = emptyMap(),
        waiting: Boolean = false,
        names: Map<Releases.Title, String> = mapOf(show to "A Show", film to "A Film"),
    ) = CalendarRules.page(
        releases = mapOf(
            show to listOf(
                release(25, Clock(21, 0), "S1 E2 · Two", "tt1:1:2"),
                release(25, null, "S1 E3 · Three", "tt1:1:3"),
                release(2, Clock(8, 0), "S1 E4 · Four", "tt1:1:4", month = 12),
            ),
            film to listOf(release(25, Clock(9, 30), "", "")),
            gone to listOf(release(25, null, "", "")),
        ),
        names = names,
        watched = { watched[it] ?: emptySet() },
        anchor = anchor, week = week, today = today, waiting = waiting,
    )

    @Test fun aDayListsItsReleasesByTimeThenName() {
        val page = page(watched = mapOf(show to setOf("tt1:1:2"), film to setOf("tt2")))
        assertEquals("November 2026", page.title)
        assertEquals(42, page.squares.size)
        assertEquals("", page.note)
        val day = page.squares.first { it.day == today }
        assertTrue(day.today && day.inMonth)
        assertEquals("Wednesday 25 November", day.longName)
        // A title off the Watchlist is left out.
        assertEquals(listOf("A Film", "A Show", "A Show"), day.entries.map { it.title })
        assertEquals(listOf("09:30", "21:00", ""), day.entries.map { it.time })
        assertEquals(listOf(true, true, false), day.entries.map { it.watched })
        assertEquals(listOf("Film", "S1 E2 · Two", "S1 E3 · Three"), day.entries.map { it.line })
        assertFalse(day.allWatched)
        // December's days on the page are dimmed but still show their releases.
        val december = page.squares.first { it.day == Day(2026, 12, 2) }
        assertFalse(december.inMonth)
        assertEquals(1, december.entries.size)
    }

    @Test fun aWeekShowsSevenDays() {
        val page = page(week = true)
        assertEquals(7, page.squares.size)
        assertTrue(page.squares.all { it.inMonth })
        assertEquals("23 – 29 November 2026", page.title)
        assertEquals("30 Nov – 6 Dec 2026", page(week = true, anchor = Day(2026, 12, 2)).title)
        assertEquals("Nothing on your Watchlist comes out this week.", page(week = true, anchor = Day(2026, 12, 9)).note)
    }

    @Test fun theNoteSaysWhyNothingShows() {
        assertEquals("Add films and series to your Watchlist to see here when they come out.", page(names = emptyMap()).note)
        assertEquals("Looking up dates…", page(waiting = true).note)
        assertEquals("Nothing on your Watchlist comes out this month.", page(anchor = Day(2027, 3, 1)).note)
    }

    @Test fun stepsGoAMonthOrAWeek() {
        assertEquals(Day(2027, 1, 1), CalendarRules.step(Day(2026, 12, 15), 1, false, today))
        assertEquals(Day(2025, 12, 1), CalendarRules.step(Day(2026, 1, 15), -1, false, today))
        assertEquals(Day(2026, 12, 2), CalendarRules.step(today, 1, true, today))
        assertEquals(today, CalendarRules.step(Day(2020, 1, 1), 0, false, today))
    }

    @Test fun theDayPickedStaysWhileItIsOnThePage() {
        val squares = page().squares
        assertEquals(Day(2026, 11, 3), CalendarRules.pick(Day(2026, 11, 3), squares, today))
        assertEquals(today, CalendarRules.pick(null, squares, today))
        val march = page(anchor = Day(2027, 3, 1)).squares
        assertEquals(Day(2027, 3, 1), CalendarRules.pick(today, march, today))
    }

    @Test fun theTimeZoneIsNamed() {
        val now = Instant.parse("2026-11-25T00:00:00Z")
        assertEquals("Times are in your local time (UTC+11:00).", CalendarRules.zoneNote(ZoneOffset.ofHours(11), now))
        assertEquals("Times are in your local time (UTC-05:30).", CalendarRules.zoneNote(ZoneOffset.ofHoursMinutes(-5, -30), now))
    }
}
