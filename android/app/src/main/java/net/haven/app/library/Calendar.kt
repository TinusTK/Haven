package net.haven.app.library

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import java.time.Instant
import java.time.ZoneId
import kotlin.math.abs

/** The Calendar page: what comes out from the Watchlist, a month or a week
 *  at a time. On a phone the day picked lists its releases. The Rust build's
 *  `library/calendar.rs` (`show_calendar`, `step_calendar`). */
object CalendarRules {
    val weekdays = listOf("Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun")
    val weekdayNames = listOf("Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday")

    /** One film or episode on a day. */
    data class Entry(
        val kind: String,
        val id: String,
        val title: String,
        /** "S3 E5 · Its name"; empty for a film. */
        val label: String,
        /** The episode's id; empty for a film. */
        val video: String,
        /** "21:00"; empty when only the date is known. */
        val time: String,
        val watched: Boolean,
    ) {
        /** The second line: the episode, "Film" or "Comes out". */
        val line: String get() = label.ifEmpty { if (kind == "movie") "Film" else "Comes out" }
    }

    /** One square of the page. */
    data class Square(
        val day: Day,
        val inMonth: Boolean,
        val today: Boolean,
        val entries: List<Entry>,
        /** "Wednesday 25 November" */
        val longName: String,
    ) {
        /** Every release shown has been watched (a tick instead of dots). */
        val allWatched: Boolean get() = entries.isNotEmpty() && entries.all { it.watched }
    }

    data class Page(
        /** "November 2026", "23 – 29 November 2026", "30 Nov – 6 Dec 2026" */
        val title: String,
        val squares: List<Square>,
        val note: String,
    )

    private class Timed(val time: Clock?, val entry: Entry)

    /** In order of time (those with none last), then name. */
    private val order = Comparator<Timed> { a, b ->
        val x = a.time
        val y = b.time
        when {
            x != null && y != null && x != y -> x.compareTo(y)
            x == null && y != null -> 1
            x != null && y == null -> -1
            else -> compareValuesBy(a.entry, b.entry, Entry::title, Entry::label)
        }
    }

    /** The page for the month or week holding `anchor`. `watched` gives a
     *  title's finished films and episodes. */
    fun page(
        releases: Map<Releases.Title, List<Release>>,
        names: Map<Releases.Title, String>,
        watched: (Releases.Title) -> Set<String>,
        anchor: Day,
        week: Boolean,
        today: Day,
        waiting: Boolean,
    ): Page {
        val byDay = HashMap<Day, MutableList<Timed>>()
        for ((title, list) in releases) {
            val name = names[title] ?: continue
            val done = watched(title)
            for (release in list) {
                val seen = if (release.video.isEmpty()) done.isNotEmpty() else release.video in done
                byDay.getOrPut(release.day) { mutableListOf() } += Timed(
                    release.time,
                    Entry(title.kind, title.id, name, release.label, release.video, release.time?.text ?: "", seen),
                )
            }
        }
        val days = if (week) Releases.week(anchor) else Releases.month(anchor.year, anchor.month)
        val squares = days.map { day ->
            Square(
                day = day,
                inMonth = week || day.month == anchor.month,
                today = day == today,
                entries = (byDay[day] ?: emptyList()).sortedWith(order).map { it.entry },
                longName = "${weekdayNames[day.weekdayFromMonday]} ${day.day} ${Releases.months[day.month - 1]}",
            )
        }
        val shown = squares.filter { it.inMonth }.flatMap { it.entries }
        val note = when {
            names.isEmpty() -> "Add films and series to your Watchlist to see here when they come out."
            waiting -> "Looking up dates…"
            shown.isEmpty() -> if (week) "Nothing on your Watchlist comes out this week." else "Nothing on your Watchlist comes out this month."
            else -> ""
        }
        return Page(title(days, anchor, week), squares, note)
    }

    fun title(days: List<Day>, anchor: Day, week: Boolean): String {
        val first = days.firstOrNull()
        val last = days.lastOrNull()
        if (!week || first == null || last == null) return "${Releases.months[anchor.month - 1]} ${anchor.year}"
        if (first.month == last.month) return "${first.day} – ${last.day} ${Releases.months[last.month - 1]} ${last.year}"
        return "${first.day} ${Releases.months[first.month - 1].take(3)} – ${last.day} ${Releases.months[last.month - 1].take(3)} ${last.year}"
    }

    /** A month (or week) back (-1) or on (1), or today (0). */
    fun step(anchor: Day, by: Int, week: Boolean, today: Day): Day {
        if (by == 0) return today
        if (week) return anchor.adding(if (by < 0) -7 else 7)
        var month = anchor.month + if (by < 0) -1 else 1
        var year = anchor.year
        if (month < 1) { month = 12; year -= 1 }
        if (month > 12) { month = 1; year += 1 }
        return Day(year, month, 1)
    }

    /** "Times are in your local time (UTC+11:00)." */
    fun zoneNote(zone: ZoneId, at: Instant = Instant.now()): String {
        val seconds = zone.rules.getOffset(at).totalSeconds
        val sign = if (seconds < 0) "-" else "+"
        val minutes = abs(seconds) / 60
        return "Times are in your local time (UTC%s%02d:%02d).".format(sign, minutes / 60, minutes % 60)
    }

    /** The day to list after the page changed: the one picked while it is
     *  still on the page, else today, else the first day of the month. */
    fun pick(before: Day?, squares: List<Square>, today: Day): Day? {
        if (before != null && squares.any { it.day == before }) return before
        if (squares.any { it.day == today }) return today
        return (squares.firstOrNull { it.inMonth } ?: squares.firstOrNull())?.day
    }
}

/** What the Calendar page has chosen: the month or week showing and the day picked. */
class CalendarModel(private val library: Library) {
    /** A day in the month or week showing. */
    var anchor by mutableStateOf(library.today())
        private set
    /** A week at a time instead of a month. */
    var week by mutableStateOf(false)
        private set
    /** The day whose releases are listed (null: today, or the month's first). */
    var picked by mutableStateOf<Day?>(null)

    val today: Day get() = library.today()

    fun chooseWeek(on: Boolean) {
        week = on
        picked = null
    }

    val page: CalendarRules.Page
        get() {
            // Read so the page draws again when dates or watched marks change.
            library.releasesChanged
            library.marks
            val names = HashMap<Releases.Title, String>()
            for (saved in library.watchlist) names.putIfAbsent(Releases.Title(saved.kind, saved.titleId), saved.title)
            return CalendarRules.page(
                library.allReleases, names, { library.finishedOf(it.kind, it.id) },
                anchor, week, today, library.calendarWaiting > 0,
            )
        }

    /** The square listed below the page. */
    fun pickedSquare(page: CalendarRules.Page): CalendarRules.Square? {
        val day = CalendarRules.pick(picked, page.squares, today)
        return page.squares.firstOrNull { it.day == day }
    }

    fun step(by: Int) {
        anchor = CalendarRules.step(anchor, by, week, today)
        if (by == 0) picked = today
    }

    /** The page opened: dates of titles not asked for yet are looked up. */
    fun appeared() {
        library.refreshCalendar()
    }
}
