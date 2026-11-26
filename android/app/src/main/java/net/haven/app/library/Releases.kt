package net.haven.app.library

import net.haven.app.titles.MetaDetail
import java.time.Duration
import java.time.Instant
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter

/** A calendar day, without a time zone. */
data class Day(val year: Int, val month: Int, val day: Int) : Comparable<Day> {
    override fun compareTo(other: Day) = compareValuesBy(this, other, Day::year, Day::month, Day::day)

    /** "2026-11-25". */
    override fun toString() = "%04d-%02d-%02d".format(year, month, day)

    val date: LocalDate get() = LocalDate.of(year, month, day)

    /** The day `count` days on (back when negative). */
    fun adding(days: Int): Day = of(date.plusDays(days.toLong()))

    /** Days from this to `other`. */
    fun daysTo(other: Day): Int = java.time.temporal.ChronoUnit.DAYS.between(date, other.date).toInt()

    /** 0 for Monday … 6 for Sunday. */
    val weekdayFromMonday: Int get() = date.dayOfWeek.value - 1

    companion object {
        fun of(date: LocalDate) = Day(date.year, date.monthValue, date.dayOfMonth)

        /** Reads "2026-11-25" (the first ten characters of a longer text). */
        fun parse(text: String): Day? {
            val head = text.take(10).split("-")
            if (head.size != 3 || head[0].length != 4) return null
            val (y, m, d) = head.map { it.toIntOrNull() ?: return null }
            return runCatching { of(LocalDate.of(y, m, d)) }.getOrNull()
        }

        /** Today where the phone is. */
        fun today(zone: ZoneId = ZoneId.systemDefault()) = of(LocalDate.now(zone))

        fun at(instant: Instant, zone: ZoneId) = of(instant.atZone(zone).toLocalDate())
    }
}

/** A time of day, in the phone's time zone. */
data class Clock(val hour: Int, val minute: Int) : Comparable<Clock> {
    override fun compareTo(other: Clock) = compareValuesBy(this, other, Clock::hour, Clock::minute)

    /** "21:00". */
    val text get() = "%02d:%02d".format(hour, minute)
}

/** One film or episode coming out. */
data class Release(
    val day: Day,
    /** The local time of day, when the source gives a time. */
    val time: Clock?,
    /** "S3 E5 · Its name" for an episode; empty for a film. */
    val label: String,
    /** The episode's own id, to open it by; empty for a film. */
    val video: String,
    /** "S03E05" for an episode; empty for a film. */
    val number: String,
    /** The episode's name, if it has one. */
    val name: String,
)

/** When things on the Watchlist come out (the Rust build's calendar rules). */
object Releases {
    val months = listOf("January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December")

    /** When something released at `released` (an ISO date, or a moment with a
     *  time zone) comes out in `zone`. A moment is moved to that zone, so an
     *  evening in America is the next day in Australia. A bare date, or
     *  midnight UTC (how sources write a date they have no time for), stays
     *  that date: there is no time to move. */
    fun local(released: String, zone: ZoneId): Pair<Day, Clock?>? {
        val text = released.trim()
        if (text.length > 10) {
            moment(text)?.let { moment ->
                val utc = moment.atOffset(ZoneOffset.UTC)
                if (utc.hour == 0 && utc.minute == 0 && utc.second == 0 && utc.nano < 1_000_000) return Day.of(utc.toLocalDate()) to null
                val local = moment.atZone(zone)
                return Day.of(local.toLocalDate()) to Clock(local.hour, local.minute)
            }
        }
        return Day.parse(text)?.let { it to null }
    }

    /** An RFC 3339 moment, with or without fractions of a second. */
    fun moment(text: String): Instant? = runCatching { OffsetDateTime.parse(text, DateTimeFormatter.ISO_OFFSET_DATE_TIME).toInstant() }.getOrNull()

    /** What a title's details say comes out, and when, in `zone`. */
    fun of(meta: MetaDetail, zone: ZoneId): List<Release> {
        // Specials (season 0) are left out: sources list making-of pieces
        // there on the same day as the real episodes.
        var releases = meta.videos.filter { it.season != 0 }.mapNotNull { video ->
            val (day, time) = video.released?.let { local(it, zone) } ?: return@mapNotNull null
            val season = video.season
            val episode = video.episode
            val (number, short) = when {
                season != null && episode != null -> "S$season E$episode" to "S%02dE%02d".format(season, episode)
                episode != null -> "E$episode" to "E%02d".format(episode)
                else -> "" to ""
            }
            val label = listOf(number, video.title ?: "").filter { it.isNotEmpty() }.joinToString(" · ")
            Release(day, time, label, video.id, short, video.title ?: "")
        }
        // The same episode listed twice (some sources repeat it) shows once.
        val seen = HashSet<String>()
        releases = releases.filter { seen.add("${it.day}|${it.label}") }
        // A film, or a series with no episodes listed: the title's own date.
        if (releases.isEmpty()) {
            meta.released?.let { local(it, zone) }?.let { (day, time) -> releases = listOf(Release(day, time, "", "", "", "")) }
        }
        return releases
    }

    /** The 7 days of the week (Monday first) that holds `day`. */
    fun week(day: Day): List<Day> {
        val start = day.adding(-day.weekdayFromMonday)
        return (0 until 7).map { start.adding(it) }
    }

    /** The 42 days a month's page shows: whole weeks from the Monday on or
     *  before the 1st. */
    fun month(year: Int, month: Int): List<Day> {
        val first = Day(year, month, 1)
        val start = first.adding(-first.weekdayFromMonday)
        return (0 until 42).map { start.adding(it) }
    }

    /** A title on the Watchlist: its kind and id. */
    data class Title(val kind: String, val id: String)

    /** One card in Home's Upcoming or Watch now row. */
    data class Card(
        val id: String,
        val kind: String,
        /** The episode to open; empty for a film. */
        val video: String,
        val title: String,
        /** The episode's name, "S03E05", "New episodes" or "Film". */
        val episode: String,
        /** "S03E05", or "S03E07–E08" when more come the same day. */
        val badge: String,
        /** "Today", "Tomorrow", "In 4 days", "Nov 5" (as built; on show,
         *  [whenAt] keeps it up to date). */
        val `when`: String,
        /** Less than a week away: the pill is gold. */
        val soon: Boolean,
        /** The title's wide picture (else its poster). */
        val picture: String?,
        val day: Day,
        val time: Clock?,
        /** On Watch now: out already, so the pill says "Watch now". */
        val out: Boolean = false,
    ) {
        /** The label at `now`: on the day it counts down by the hour. */
        fun whenAt(now: Instant, zone: ZoneId = ZoneId.systemDefault()) = if (out) "Watch now" else whenLabel(day, time, now, zone)
    }

    /** How soon something out on `day` (at `time`, when known) comes out,
     *  seen at `now`: "Nov 5", "In 6 days", "Tomorrow", and on the day
     *  "In 5 h", "In 40 min", "Out now" (or "Today" with no time). */
    fun whenLabel(day: Day, time: Clock?, now: Instant, zone: ZoneId = ZoneId.systemDefault()): String {
        val days = Day.at(now, zone).daysTo(day)
        return when {
            days <= 0 -> {
                time ?: return "Today"
                val out = LocalDateTime.of(day.year, day.month, day.day, time.hour, time.minute).atZone(zone).toInstant()
                val minutes = Duration.between(now, out).seconds / 60
                when {
                    minutes <= 0 -> "Out now"
                    minutes < 60 -> "In $minutes min"
                    // Whole hours left: 4 h 50 min reads "In 4 h".
                    else -> "In ${minutes / 60} h"
                }
            }
            days == 1 -> "Tomorrow"
            days in 2..6 -> "In $days days"
            else -> "${months[day.month - 1].take(3)} ${day.day}"
        }
    }

    /** What of a title has come out and is still to watch (Watch now). */
    data class OutNow(val title: Title, val first: Release, val last: Release, val count: Int)

    private fun before(a: Release, b: Release): Boolean {
        if (a.day != b.day) return a.day < b.day
        if (a.time != b.time) {
            val x = a.time ?: return true
            val y = b.time ?: return false
            return x < y
        }
        return a.number < b.number
    }

    private val order = Comparator<Release> { a, b -> if (before(a, b)) -1 else if (before(b, a)) 1 else 0 }

    /** Home's Watch now row, as the Rust build's `out_now`: for each Watchlist
     *  title, the films and episodes out before `today` and not watched,
     *  either since it went on the Watchlist (`added`) or after the last one
     *  watched (two episodes left of a season show up). One a title, until
     *  all are watched; newest first. */
    fun outNow(releases: Map<Title, List<Release>>, added: Map<Title, Day>, watched: (Title, Release) -> Boolean, today: Day): List<OutNow> {
        val out = releases.mapNotNull { (title, list) ->
            val since = added[title] ?: return@mapNotNull null
            val reached = list.filter { watched(title, it) }.maxWithOrNull(order)
            val left = list.filter { release ->
                release.day < today && !watched(title, release) &&
                    (release.day >= since || (reached != null && before(reached, release)))
            }.sortedWith(order)
            if (left.isEmpty()) null else OutNow(title, left.first(), left.last(), left.size)
        }
        return out.sortedWith { a, b ->
            if (a.last.day != b.last.day || a.last.time != b.last.time) {
                if (before(b.last, a.last)) -1 else 1
            } else {
                a.title.id.compareTo(b.title.id)
            }
        }
    }

    /** "S01E03–E05", or "S01E09–S02E01" across seasons, for a run of episodes. */
    fun runBadge(first: String, last: String): String = when {
        first == last || last.isEmpty() -> first
        first.length > 3 && last.length > 3 && first.take(3) == last.take(3) -> "$first–${last.drop(3)}"
        else -> "$first–$last"
    }

    /** Watch now's cards, at most 12. */
    fun watchNow(
        releases: Map<Title, List<Release>>,
        names: Map<Title, String>,
        pictures: Map<Title, String>,
        added: Map<Title, Day>,
        watched: (Title, Release) -> Boolean,
        today: Day,
    ): List<Card> = outNow(releases, added, watched, today).mapNotNull { out ->
        val name = names[out.title] ?: return@mapNotNull null
        val episode = when {
            out.count > 1 -> "${out.count} new episodes"
            out.first.name.isNotEmpty() -> out.first.name
            out.first.number.isEmpty() -> "Film"
            else -> out.first.number
        }
        Card(out.title.id, out.title.kind, out.first.video, name, episode, runBadge(out.first.number, out.last.number),
            "Watch now", true, pictures[out.title], out.last.day, out.last.time, out = true)
    }.take(12)

    /** Home's Upcoming row: from today on, one card a show (its next release;
     *  more of it the same day widen the badge), at most 12. */
    fun upcoming(
        releases: Map<Title, List<Release>>,
        names: Map<Title, String>,
        pictures: Map<Title, String>,
        today: Day,
        now: Instant = Instant.now(),
        zone: ZoneId = ZoneId.systemDefault(),
    ): List<Card> {
        var coming = releases.flatMap { (title, list) -> list.map { title to it } }.filter { it.second.day >= today }
        coming = coming.sortedWith { (ta, a), (tb, b) ->
            when {
                a.day != b.day -> a.day.compareTo(b.day)
                // A release with no time comes before one with a time (Rust's None < Some).
                a.time != null && b.time != null && a.time != b.time -> a.time.compareTo(b.time)
                a.time == null && b.time != null -> -1
                a.time != null && b.time == null -> 1
                else -> ta.id.compareTo(tb.id)
            }
        }
        // The last release of each show on its first day.
        val firstDay = HashMap<Title, Day>()
        val lastSameDay = HashMap<Title, Release>()
        for ((title, release) in coming) {
            if (title !in firstDay) firstDay[title] = release.day
            if (firstDay[title] == release.day) lastSameDay[title] = release
        }
        val seen = HashSet<Title>()
        coming = coming.filter { seen.add(it.first) }
        return coming.mapNotNull { (title, release) ->
            val name = names[title] ?: return@mapNotNull null
            val last = lastSameDay[title]?.takeIf { it.number != release.number }
            val days = today.daysTo(release.day)
            val `when` = when {
                days == 0 -> if (release.time == null) "Today" else whenLabel(release.day, release.time, now, zone)
                days == 1 -> "Tomorrow"
                days in 2..6 -> "In $days days"
                else -> "${months[release.day.month - 1].take(3)} ${release.day.day}"
            }
            val episode = when {
                last != null -> "New episodes"
                release.name.isNotEmpty() -> release.name
                release.number.isEmpty() -> "Film"
                else -> release.number
            }
            val badge = if (last != null && last.number.length > 3) "${release.number}–${last.number.takeLast(3)}" else release.number
            Card(title.id, title.kind, release.video, name, episode, badge, `when`, days < 7, pictures[title], release.day, release.time)
        }.take(12)
    }
}
