package net.haven.app.library

import net.haven.app.core.Download
import net.haven.app.core.WatchProgress
import net.haven.app.player.Resume
import java.io.File
import java.util.Locale

/** One line of the Library's Downloads list. */
data class DownloadRow(
    val id: Long,
    /** "Films" or "Series" on the first row of its section; empty after. */
    val section: String,
    /** A series' name on its first episode; empty otherwise. */
    val group: String,
    val title: String,
    val status: String,
    /** 0 to 1. */
    val progress: Double,
    val done: Boolean,
    val running: Boolean,
    val failed: Boolean,
    /** The episode's picture (a film's backdrop), kept with the download. */
    val picture: File? = null,
)

/** A video on this device (Library › Local): from Android's media list. */
data class FoundVideo(
    val name: String,
    /** The folder it is in ("Camera", "Movies"). */
    val place: String,
    val size: Long,
    /** Seconds since 1970. */
    val modified: Long,
    /** What the player opens (`content://…`). */
    val address: String,
) {
    /** "Movies  ·  1.2 GB" */
    val detail get() = listOf(place, if (size > 0) DownloadRules.localSize(size) else "").filter { it.isNotEmpty() }.joinToString("  ·  ")
}

/** Where downloads go, and how they are named and listed. The Rust build's
 *  `library/downloads.rs` and `library/local.rs` rules. */
object DownloadRules {
    /** Downloads fetched at the same time; the rest wait their turn. */
    const val AT_ONCE = 2
    /** How often a download's progress reaches the screen and the database. */
    const val REPORT_EVERY_MILLIS = 700L
    /** The most videos Local lists. */
    const val MOST_VIDEOS = 1000

    const val LOCAL_PLACE = "Videos on this device"
    const val NOTHING_DOWNLOADED = "Nothing downloaded yet. On a film's or an episode's page, press the download button beside a place to watch: it is saved to this device and shows here, sorted into films and series."
    const val NO_LOCAL_VIDEOS = "No videos found on this device."
    const val ALLOW_LOCAL = "Allow Haven to see the videos on this device to list them here."

    /** Haven's folder for downloads: the device's Movies folder (where other
     *  apps see them too, as the Rust build kept them), else Haven's own. */
    fun root(movies: File?, data: File): File {
        val shared = movies?.let { File(it, "Haven") }
        if (shared != null && (shared.isDirectory || shared.mkdirs())) return shared
        return File(data, "Downloads")
    }

    /** Where downloads go, in words for the screen. */
    fun placeWords(root: File) = if (root.path.contains("/Movies/Haven")) "Movies › Haven on this device" else "Haven's own folder on this device"

    /** A name that is safe as a file or folder name everywhere. */
    fun safeName(name: String): String {
        val cleaned = name.map { if (Character.isISOControl(it) || it in "<>:\"/\\|?*") ' ' else it }.joinToString("")
        val joined = cleaned.split(Regex("\\s+")).filter { it.isNotEmpty() }.joinToString(" ")
        val trimmed = joined.trim { it == '.' || it == ' ' }
        return if (trimmed.isEmpty()) "Untitled" else trimmed.take(120)
    }

    /** The file type, from the address (`mkv`, `mp4`…), else `mkv`. */
    fun extensionOf(address: String): String {
        val path = address.split('?', '#').first().lowercase()
        return listOf("mkv", "mp4", "m4v", "avi", "mov", "webm", "ts", "wmv", "mpg").firstOrNull { path.endsWith(".$it") } ?: "mkv"
    }

    /** The file's name: `Dune.mkv`, or `Show - S01E02 - Its name.mkv`. */
    fun fileName(d: Download, extension: String): String {
        val show = safeName(d.title)
        val season = d.season
        if (d.kind == "movie" || season == null) return "$show.$extension"
        val number = d.episode?.let { "S%02dE%02d".format(season, it) } ?: "S%02d".format(season)
        // "S1 E2 · Its name" gives the episode's name after the dot.
        val mark = d.detail.indexOf(" · ")
        return if (mark >= 0) "$show - $number - ${safeName(d.detail.substring(mark + 3))}.$extension" else "$show - $number.$extension"
    }

    /** Where a download is saved, relative to the downloads folder:
     *  `Movies/<film>.mkv` or `Series/<show>/Season <n>/<episode>.mkv`. */
    fun relativePath(d: Download, extension: String): String {
        val file = fileName(d, extension)
        val season = d.season
        return if (season != null && d.kind != "movie") "Series/${safeName(d.title)}/Season $season/$file" else "Movies/$file"
    }

    fun sizeWords(bytes: Long): String {
        val b = maxOf(bytes, 0L).toDouble()
        return when {
            b >= 1e9 -> String.format(Locale.ROOT, "%.1f GB", b / 1e9)
            b >= 1e6 -> String.format(Locale.ROOT, "%.0f MB", b / 1e6)
            else -> String.format(Locale.ROOT, "%.0f KB", maxOf(b / 1e3, 1.0))
        }
    }

    fun localSize(bytes: Long): String {
        val mb = 1024.0 * 1024
        return if (bytes >= 1024 * mb) String.format(Locale.ROOT, "%.1f GB", bytes / (1024 * mb)) else String.format(Locale.ROOT, "%.0f MB", bytes / mb)
    }

    /** The Downloads list: films, then series, each series under its name
     *  with its episodes in order (the order the database gives). */
    fun rows(all: List<Download>, live: Map<Long, Pair<Long, Long>>, picture: (Long) -> File? = { null }): List<DownloadRow> {
        val rows = mutableListOf<DownloadRow>()
        var lastSection = ""
        var lastShow = ""
        for (d in all) {
            val film = d.kind == "movie" || d.season == null
            val section = if (film) "Films" else "Series"
            val (size, received) = live[d.id] ?: (d.size to d.received)
            val progress = if (d.state == "done") 1.0 else if (size > 0) received.toDouble() / size else 0.0
            val percent = Math.round(progress * 100)
            val status = when (d.state) {
                "done" -> sizeWords(maxOf(size, received))
                "paused" -> "Paused · $percent%"
                "failed" -> "Stopped: could not fetch it. Try again."
                "waiting" -> "Waiting…"
                else -> if (size > 0) "$percent% · ${sizeWords(received)} of ${sizeWords(size)}" else "${sizeWords(received)} so far"
            }
            rows += DownloadRow(
                d.id, if (section != lastSection) section else "", if (!film && d.title != lastShow) d.title else "",
                if (film || d.detail.isEmpty()) d.title else d.detail, status, progress, d.state == "done", d.state == "loading",
                d.state == "failed", picture(d.id),
            )
            lastSection = section
            if (!film) lastShow = d.title
        }
        return rows
    }

    /** "1 video.", "12 videos.", or that there are none. */
    fun localStatus(count: Int) = when (count) {
        0 -> NO_LOCAL_VIDEOS
        1 -> "1 video."
        else -> "$count videos."
    }

    /** The line under a History poster: the episode, "Watched", or how much is left. */
    fun historyNote(p: WatchProgress): String = when {
        p.detail.isNotEmpty() -> p.detail
        p.finished -> "Watched"
        else -> "${Resume.clock(p.duration - p.position)} left"
    }
}
