package net.haven.app.library

import net.haven.app.BuildConfig
import net.haven.app.metadata.Tmdb
import net.haven.app.player.PlayableAddress
import net.haven.app.servers.Servers
import net.haven.app.servers.Wanted
import net.haven.app.titles.MediaStream
import net.haven.app.titles.MetaDetail
import net.haven.app.titles.MetaPreview
import net.haven.app.titles.StreamKind
import net.haven.app.titles.Video
import java.math.BigDecimal
import java.math.RoundingMode

/** Everything the player needs to play a stream and remember the place. */
data class PlayRequest(
    val address: String,
    val title: String,
    /** "S1 E2 · Name"; empty for a film. */
    val detail: String,
    val kind: String,
    val titleId: String,
    val videoId: String,
    val poster: String?,
    /** Where it plays from, in words, for the player's information. */
    val source: String,
    /** Remember how far it was watched (films and episodes, not channels). */
    val remember: Boolean = true,
    /** The episode after this one, in words; null if there is none. */
    val next: String? = null,
    /** The media server item (`srv:…`) it plays, told how far it was watched. */
    val serverItem: String? = null,
)

/** One episode in the title page's list. */
data class EpisodeRow(
    val id: String,
    /** "E5"; empty when it has no number. */
    val number: String,
    val title: String,
    /** "2019-01-27" */
    val date: String,
    val selected: Boolean = false,
    /** Watched to the end. */
    val watched: Boolean = false,
    /** A still from the episode, if the source has one. */
    val picture: String? = null,
)

/** One place to watch in the title page's list. */
data class StreamRow(
    /** Stays the same while rows are added around it. */
    val id: Int,
    /** The media server or add-on that answered. */
    val source: String,
    val name: String,
    val detail: String,
    /** The rest of the detail, on a second line. */
    val detail2: String,
    /** The file's size ("12.3 GB"); empty if unknown. */
    val size: String,
    /** How many are sharing it; empty if unknown. */
    val peers: String,
    /** How it is delivered: "Direct", "Web page", "YouTube", "Peer to peer". */
    val kind: String,
    /** What Haven's player opens; null when Haven cannot play the row. */
    val address: String? = null,
    /** Who provided it: an add-on's name, or for a media server the add-on it
     *  got the stream from (else the server itself). */
    val group: String,
    /** The media server item (`srv:…`) the row plays, for a server's row. */
    val serverItem: String? = null,
    // Its place under its provider's heading.
    val groupAt: Int = 0,
    val shown: Boolean = true,
    /** The provider's heading, on its first row only. */
    val heading: String = "",
    /** How many more the provider has, on its last shown row only. */
    val more: Int = 0,
) {
    val playable get() = address != null
}

/** The title page's rules, ported from the Rust build (`library.rs`,
 *  `library/sources.rs`, `library/marks.rs`). */
object TitleRules {
    /** The heading over the list of places to watch. */
    const val HEADING = "Play from your sources"
    /** Places to watch shown per provider at first, and added by "Show more". */
    const val STREAMS_SHOWN = 10
    /** Names shown under "Cast". */
    const val CAST_SHOWN = 6

    // The Play build has add-ons only through the Stremio sign-in.
    val noDetails = if (BuildConfig.ADDON_SCREENS) "None of your add-ons has details for this title." else "There are no details for this title."
    val nothingCanPlay = if (BuildConfig.ADDON_SCREENS) {
        "Nothing you have added can play this. Add a media server or an Xtream Codes account in Settings, or install an add-on that offers streams."
    } else {
        "Nothing you have added can play this. Add a media server or an Xtream Codes account in Settings."
    }
    const val NONE_HAS_IT = "None of your media servers, accounts or add-ons has this."
    const val ASKING = "Asking your media servers and add-ons…"
    const val NO_TMDB_KEY = "Add a TMDB key in Settings to see this title's details."
    const val CHOOSE_EPISODE = "Choose an episode to see where you can watch it."

    // Details

    /** Year, runtime, rating and genres on one line. */
    fun facts(meta: MetaDetail): String = buildList {
        meta.releaseInfo?.let(::add)
        meta.runtime?.let(::add)
        meta.rating?.let { add("★ $it") }
        if (meta.genres.isNotEmpty()) add(meta.genres.take(3).joinToString(", "))
    }.joinToString("  ·  ")

    fun episodeRow(video: Video) = EpisodeRow(
        video.id, video.episode?.let { "E$it" } ?: "", video.title ?: video.episode?.let { "Episode $it" } ?: "Untitled",
        video.released?.take(10) ?: "", picture = video.thumbnail,
    )

    /** "S1 E2 · Its name", as playing an episode records it. */
    fun episodeDetail(video: Video): String {
        val number = when {
            video.season != null && video.episode != null -> "S${video.season} E${video.episode}"
            video.episode != null -> "E${video.episode}"
            else -> ""
        }
        return listOf(number, video.title ?: "").filter { it.isNotEmpty() }.joinToString(" · ")
    }

    /** Has come out by `today` (an ISO date); no date counts as out. */
    fun aired(video: Video, today: String): Boolean = video.released?.take(10)?.let { it <= today } ?: true

    /** The episode after `current`, in the order they are watched (specials
     *  are not in the run). */
    fun nextVideo(meta: MetaDetail, current: String): Video? {
        val run = meta.videos.filter { (it.season ?: 1) > 0 }
            .sortedWith(compareBy({ it.season ?: Int.MIN_VALUE }, { it.episode ?: Int.MIN_VALUE }))
        val at = run.indexOfFirst { it.id == current }
        return if (at < 0 || at + 1 >= run.size) null else run[at + 1]
    }

    // Who is asked

    /** Describes the film or episode `videoId` of the open title, so media
     *  servers can be asked whether they have it. */
    fun wanted(kind: String, videoId: String, openId: String, meta: MetaDetail?, name: String): Wanted {
        // The IMDb id: named by the details, or the title's own id, or the
        // start of an episode id such as `tt123:1:2`.
        fun imdbOrNull(id: String?) = id?.takeIf { Tmdb.isImdbId(it) }
        val imdb = meta?.imdbId ?: imdbOrNull(meta?.id) ?: imdbOrNull(openId) ?: imdbOrNull(videoId.substringBefore(":"))
        val tmdb = meta?.tmdbId ?: Tmdb.splitId(openId)?.second?.toString()
        val video = meta?.videos?.firstOrNull { it.id == videoId }
        val episode = if (video?.season != null && video.episode != null) video.season to video.episode else null
        return Wanted(kind == "series", meta?.name ?: name, meta?.releaseInfo, imdb, tmdb, episode)
    }

    /** What add-ons know the film or episode as: its own id if it came from an
     *  add-on, else the IMDb form (`tt…`, `tt…:season:episode`). */
    fun addonId(videoId: String, wanted: Wanted): String? {
        val foreign = Servers.splitId(videoId) != null || videoId.startsWith("tmdb:")
        if (!foreign) return videoId
        val imdb = wanted.imdb ?: return null
        if (!wanted.series) return imdb
        val (season, number) = wanted.episode ?: return null
        return "$imdb:$season:$number"
    }

    /** The IMDb id of the title in `found` (a catalogue search) with the
     *  wanted name and, when both are known, year. */
    fun imdbMatch(found: List<MetaPreview>, name: String, year: String?): String? {
        fun plain(text: String) = text.filter { it.isLetterOrDigit() }.lowercase()
        fun yearOf(text: String?) = text?.takeIf { it.length >= 4 }?.take(4)?.toIntOrNull()
        val wantedName = plain(name)
        if (wantedName.isEmpty()) return null
        return found.firstOrNull { title ->
            if (!Tmdb.isImdbId(title.id) || plain(title.name) != wantedName) return@firstOrNull false
            val a = yearOf(year)
            val b = yearOf(title.releaseInfo)
            if (a != null && b != null) a == b else true
        }?.id
    }

    // Rows

    /** A row for one stream, or null if it names no source. */
    fun streamRow(id: Int, source: String, stream: MediaStream): StreamRow? {
        val kind = stream.kind ?: return null
        val label = when (kind) {
            StreamKind.Direct -> "Direct"
            StreamKind.External -> "Web page"
            StreamKind.YouTube -> "YouTube"
            StreamKind.PeerToPeer -> "Peer to peer"
        }
        fun oneLine(text: String) = text.split(Regex("\\s+")).filter { it.isNotEmpty() }.joinToString(" ")
        // The detail goes on two lines: the release's name, then its facts.
        // The size and how many share it are taken out and shown on their own.
        var detail = oneLine(stream.details ?: "")
        var size = stream.videoSize?.let(::sizeText)
        sizeIn(detail)?.let { (text, range) ->
            size = size ?: text
            detail = tidy(detail.removeRange(range))
        }
        var peers = ""
        peersIn(detail)?.let { (count, range) ->
            peers = count
            detail = tidy(detail.removeRange(range))
        }
        val parts = detail.split(" · ")
        var first = ""
        var used = 0
        for (part in parts) {
            if (used > 0 && first.length + part.length > 46) break
            if (used > 0) first += " · "
            first += part
            used += 1
        }
        val address = if (kind == StreamKind.Direct) stream.url?.let(PlayableAddress::web) else null
        return StreamRow(
            id, source, oneLine(stream.name ?: "Stream"), first, parts.drop(used).joinToString(" · "), size ?: "", peers, label,
            address, stream.provider ?: source,
        )
    }

    /** A size in bytes as people read it: "12.3 GB", "850 MB". */
    fun sizeText(bytes: Long): String {
        val gb = bytes / 1_073_741_824.0
        return if (gb >= 1) "${BigDecimal(gb).setScale(1, RoundingMode.HALF_EVEN).toPlainString()} GB"
        else "${BigDecimal(bytes / 1_048_576.0).setScale(0, RoundingMode.HALF_EVEN).toPlainString()} MB"
    }

    private const val FLOPPY = "💾"

    /** The first size written in a text ("12.3 GB", "850MB", "1,4 GiB") as
     *  Haven shows it, and where it is (a 💾 before it counts as part of it). */
    fun sizeIn(text: String): Pair<String, IntRange>? {
        val units = listOf("TIB" to "TB", "TB" to "TB", "GIB" to "GB", "GB" to "GB", "MIB" to "MB", "MB" to "MB")
        fun digit(at: Int) = at < text.length && text[at] in '0'..'9'
        fun alnum(at: Int) = text[at].code < 128 && text[at].isLetterOrDigit()
        var at = 0
        while (at < text.length) {
            if (!digit(at) || (at > 0 && alnum(at - 1))) {
                at += 1
                continue
            }
            var end = at
            while (digit(end) || ((text.getOrNull(end) == '.' || text.getOrNull(end) == ',') && digit(end + 1))) end += 1
            val number = text.substring(at, end).replace(',', '.')
            var unitAt = end
            while (text.getOrNull(unitAt) == ' ') unitAt += 1
            val rest = text.substring(unitAt).uppercase()
            val unit = units.firstOrNull { (written, _) ->
                rest.startsWith(written) && rest.getOrNull(written.length)?.let { it.code < 128 && it.isLetter() } != true
            }
            if (unit != null) {
                var start = at
                val before = text.substring(0, at).trimEnd()
                if (before.endsWith(FLOPPY)) start = before.length - FLOPPY.length
                return "$number ${unit.second}" to (start until unitAt + unit.first.length)
            }
            at = maxOf(end, at + 1)
        }
        return null
    }

    /** How many share the file, where a detail line names it with a people
     *  sign ("👥 418", "👤 50"), and where that is. */
    fun peersIn(text: String): Pair<String, IntRange>? {
        for (mark in listOf("👥", "👤")) {
            val at = text.indexOf(mark).takeIf { it >= 0 } ?: continue
            var from = at + mark.length
            while (text.getOrNull(from)?.let { it == ' ' || it == ':' || it == '️' } == true) from += 1
            var end = from
            while (text.getOrNull(end)?.let { it in '0'..'9' || it == ',' } == true) end += 1
            val count = text.substring(from, end).replace(",", "")
            if (count.isNotEmpty()) return count to (at until end)
        }
        return null
    }

    /** Cleans up a detail line once something was taken out of it: no doubled
     *  spaces or separators, none left at either end. */
    fun tidy(text: String): String {
        val joined = text.split(Regex("\\s+")).filter { it.isNotEmpty() }.joinToString(" ")
        val parts = joined.split(" · ").map { it.trim() }.filter { it.isNotEmpty() && it != "·" }
        return parts.joinToString(" · ").trim { it == '·' || it == '|' || it.isWhitespace() }
    }

    /** Where a new row goes: your own servers stay above the add-ons, and each
     *  provider's rows stay together. */
    fun place(row: StreamRow, rows: List<StreamRow>, serverRows: Int, fromServer: Boolean): Int {
        val (from, to) = if (fromServer) 0 to serverRows else serverRows to rows.size
        if (from >= to) return to
        val last = (from until to).reversed().firstOrNull { rows[it].group == row.group } ?: return to
        return last + 1
    }

    /** Each row's place under its provider's heading: which rows show (ten per
     *  provider, more on asking), which carries the heading and which the
     *  "Show more". */
    fun marked(rows: List<StreamRow>, limits: Map<String, Int>): List<StreamRow> {
        val counts = rows.groupingBy { it.group }.eachCount()
        val places = HashMap<String, Int>()
        return rows.map { row ->
            val place = places[row.group] ?: 0
            val count = counts[row.group] ?: 0
            val limit = limits[row.group] ?: STREAMS_SHOWN
            places[row.group] = place + 1
            row.copy(
                groupAt = place,
                shown = place < limit,
                heading = if (place == 0) "${row.group}  ·  $count" else "",
                more = if (place == limit - 1 && count > limit) count - limit else 0,
            )
        }
    }

    /** The providers in the order they first appear, each with how many rows. */
    fun groups(rows: List<StreamRow>): List<Pair<String, Int>> {
        val groups = LinkedHashMap<String, Int>()
        for (row in rows) groups[row.group] = (groups[row.group] ?: 0) + 1
        return groups.toList()
    }
}
