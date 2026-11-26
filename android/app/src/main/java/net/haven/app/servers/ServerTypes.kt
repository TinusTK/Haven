package net.haven.app.servers

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import net.haven.app.core.Http
import net.haven.app.core.StoredServer
import net.haven.app.titles.JObj
import net.haven.app.titles.ResourceUrl
import java.math.BigDecimal
import java.math.RoundingMode

// Media servers: the user's own Jellyfin, Emby and Plex servers, shown in
// the same shapes catalogues use (posters, details, streams) so the rest of
// Haven does not care where a title comes from. A title on a server has the
// id `srv:<server>:<item>`, `<server>` being its number in the database.
// The Android builds have no home-network limit (as the Rust Android app).

sealed class ServerError(message: String) : Exception(message) {
    class InvalidAddress(val address: String) : ServerError("\"$address\" is not a server address")
    class Request(why: String) : ServerError("could not reach the server: $why")
    class SignInRefused : ServerError("the user name or password was not accepted")
    class Status(val code: Int) : ServerError("the server answered with error $code")
    class BadAnswer : ServerError("the server's answer could not be read")
    class TooLarge : ServerError("the server's answer was too large")
    class Unreachable : ServerError("none of this Plex server's addresses could be reached")

    override fun equals(other: Any?): Boolean =
        other is ServerError && other::class == this::class && other.message == message

    override fun hashCode(): Int = message.hashCode()
}

enum class ServerKind(val raw: String, val label: String) {
    Jellyfin("jellyfin", "Jellyfin"),
    Emby("emby", "Emby"),
    Plex("plex", "Plex");

    companion object {
        fun of(raw: String): ServerKind? = entries.firstOrNull { it.raw == raw }
    }
}

/** A server the user signed in to. Never prints its token. */
data class Server(
    /** Its number in Haven's database; 0 until it is saved. */
    val id: Long = 0,
    val kind: ServerKind,
    val name: String,
    /** Where it answers, without a trailing slash. */
    val address: String,
    /** The signed-in user's id on the server (unused for Plex). */
    val userId: String = "",
    /** What the server gave in exchange for signing in. */
    val token: String,
) {
    val stored: StoredServer get() = StoredServer(id, kind.raw, name, address, userId, token)

    override fun toString(): String = "Server(id=$id, kind=${kind.raw}, name=$name, address=$address, ..)"

    companion object {
        /** A stored server, if its kind is one Haven knows. */
        fun of(stored: StoredServer): Server? =
            ServerKind.of(stored.kind)?.let { Server(stored.id, it, stored.name, stored.address, stored.userId, stored.token) }
    }
}

/** One of a server's libraries that holds films or series. */
@Serializable
data class MediaLibrary(
    val id: String,
    val name: String,
    /** A collection (a list someone put together) and not a whole library. */
    val collection: Boolean,
)

/** A film or episode to look for on a server: the one Haven has open from
 *  somewhere else (another server, a catalogue, TMDB). */
data class Wanted(
    val series: Boolean = false,
    val name: String = "",
    /** The year it came out; for a series the year it began. */
    val year: String? = null,
    val imdb: String? = null,
    val tmdb: String? = null,
    /** For an episode: its season and its number in the season. */
    val episode: Pair<Int, Int>? = null,
)

/** A title on a server that might be the wanted one. */
data class Candidate(val id: String, val name: String, val year: String? = null, val imdb: String? = null, val tmdb: String? = null)

/** Rules shared by every kind of server. */
object Servers {
    /** Titles fetched at once for a shelf of a whole library. */
    const val LIST_LENGTH = 300
    const val ANSWER_LIMIT = 16 * 1024 * 1024

    /** The id Haven uses for an item on a server. */
    fun titleId(server: Long, item: String): String = "srv:$server:$item"

    /** Splits a `srv:<server>:<item>` id; null for any other id. */
    fun splitId(id: String): Pair<Long, String>? {
        if (!id.startsWith("srv:")) return null
        val rest = id.substring(4)
        val colon = rest.indexOf(':').takeIf { it >= 0 } ?: return null
        val server = rest.substring(0, colon).toLongOrNull() ?: return null
        val item = rest.substring(colon + 1)
        return if (item.isEmpty()) null else server to item
    }

    /** Tidies an address the user typed: adds `http://` when no scheme is
     *  given and drops trailing slashes. Only http and https are accepted. */
    fun address(typed: String): String {
        val trimmed = typed.trim()
        val withScheme = if (trimmed.contains("://")) trimmed else "http://$trimmed"
        val url = runCatching { Http.webUrl(withScheme) }.getOrNull() ?: throw ServerError.InvalidAddress(trimmed)
        if (url.host.isEmpty()) throw ServerError.InvalidAddress(trimmed)
        return url.toString().trimEnd('/')
    }

    /** Percent-encodes text for use in an address. */
    fun query(text: String): String = ResourceUrl.encoded(text)

    /** Letters and digits only, lower case: "The Film!" and "the film" agree. */
    fun plain(name: String): String = name.filter { it.isLetterOrDigit() }.lowercase()

    /** The first four digits: "2008-2013" and "2008" agree. */
    fun yearOf(text: String?): String? = text?.take(4)?.takeIf { it.length == 4 && it.all { c -> c in '0'..'9' } }

    /** Picks the candidate that is the wanted title. Shared ids decide; if
     *  the two sides share no kind of id, the name and the year must agree. */
    fun bestMatch(wanted: Wanted, candidates: List<Candidate>): Candidate? {
        fun same(a: String?, b: String?): Boolean? = if (a == null || b == null) null else a.equals(b, ignoreCase = true)
        candidates.firstOrNull { same(wanted.imdb, it.imdb) == true || same(wanted.tmdb, it.tmdb) == true }?.let { return it }
        return candidates.firstOrNull { candidate ->
            // An id of the same kind that differs means another title.
            if (same(wanted.imdb, candidate.imdb) != null || same(wanted.tmdb, candidate.tmdb) != null) return@firstOrNull false
            if (wanted.name.isEmpty() || plain(wanted.name) != plain(candidate.name)) return@firstOrNull false
            val a = yearOf(wanted.year)
            val b = yearOf(candidate.year)
            if (a != null && b != null) a == b else true
        }
    }

    /** A server that gathers titles from elsewhere (Trellis) names an item by
     *  what its source calls it: `<m or s>|<the source's id>|<type>` in hex. */
    fun gatheredParts(item: String): Triple<String, String, String>? {
        val spelt = unhex(item) ?: return null
        val parts = spelt.split("|")
        if (parts.size != 3 || (parts[0] != "m" && parts[0] != "s") || parts[1].isEmpty()) return null
        return Triple(parts[0], parts[1], parts[2])
    }

    /** Hex back to text; null if it is not hex of UTF-8 text. */
    fun unhex(text: String): String? {
        if (text.length % 2 != 0 || text.any { it.code >= 128 }) return null
        val bytes = ByteArray(text.length / 2)
        for (i in bytes.indices) bytes[i] = (text.substring(i * 2, i * 2 + 2).toIntOrNull(16) ?: return null).toByte()
        val decoder = Charsets.UTF_8.newDecoder()
        return runCatching { decoder.decode(java.nio.ByteBuffer.wrap(bytes)).toString() }.getOrNull()
    }

    fun hex(text: String): String = text.toByteArray(Charsets.UTF_8).joinToString("") { "%02x".format(it) }

    /** True for a gathered title not named by its IMDb id. */
    fun wantsImdb(item: String): Boolean = gatheredParts(item)?.let { !it.second.startsWith("tt") } ?: false

    /** The IMDb id a gathered title's id spells, if it goes by one. */
    fun imdbIn(item: String): String? = gatheredParts(item)?.second?.takeIf { it.startsWith("tt") }

    /** The same gathered title named by its IMDb id; null if `item` is not a
     *  gathered title's id or already goes by one. */
    fun byImdb(item: String, imdb: String): String? {
        val parts = gatheredParts(item) ?: return null
        if (parts.second.startsWith("tt") || !imdb.startsWith("tt")) return null
        return hex("${parts.first}|$imdb|${parts.third}")
    }

    /** One decimal, a half to even (as printf). */
    fun fixed(value: Double, places: Int): String = BigDecimal(value).setScale(places, RoundingMode.HALF_EVEN).toPlainString()

    /** A picture's height in words: "4K", "1080p", "720p"… */
    fun quality(width: Double?, height: Double?): String? {
        val w = width ?: 0.0
        val h = height ?: 0.0
        if (w <= 0 && h <= 0) return null
        if (w >= 3800 || h >= 2000) return "4K"
        if (w >= 1900 || h >= 1000) return "1080p"
        if (w >= 1260 || h >= 700) return "720p"
        if (h > 0) return "${fixed(h, 0)}p"
        return "SD"
    }

    /** A file size in words: "4.2 GB", "700 MB"; nothing under 1 MB. */
    fun size(bytes: Double): String? {
        val mb = 1024.0 * 1024.0
        if (bytes < mb) return null
        if (bytes >= 1024 * mb) return "${fixed(bytes / (1024 * mb), 1)} GB"
        return "${fixed(bytes / mb, 0)} MB"
    }
}

/** Reading server answers: text (trimmed; a number as text), numbers (or
 *  text that reads as one) and lists. Swift `ServerJSON`. */
object SJ {
    fun field(value: JsonElement?, key: String): JsonElement? = (value as? JsonObject)?.get(key)

    fun text(value: JsonElement?, key: String): String? {
        val p = field(value, key) as? JsonPrimitive ?: return null
        if (p.isString) return p.content.trim().takeIf { it.isNotEmpty() }
        if (JObj.isBool(p) || p.content == "null") return null
        return JObj.text(p)
    }

    fun number(value: JsonElement?, key: String): Double? {
        val p = field(value, key) as? JsonPrimitive ?: return null
        if (p.isString) return p.content.trim().toDoubleOrNull()
        if (JObj.isBool(p)) return null
        return p.content.toDoubleOrNull()
    }

    fun list(value: JsonElement?, key: String): List<JsonElement> = (field(value, key) as? JsonArray) ?: emptyList()

    fun obj(value: JsonElement?, key: String): JsonObject? = field(value, key) as? JsonObject

    fun bool(value: JsonElement?, key: String): Boolean? =
        (field(value, key) as? JsonPrimitive)?.takeIf { JObj.isBool(it) }?.content?.let { it == "true" }

    fun rating(value: Double): String = Servers.fixed(value, 1)

    /** A season or episode number, as Rust's `as u32` turned a number: not a
     *  number or below zero is 0, a fraction is cut off. */
    fun whole(value: Double): Int {
        if (!value.isFinite() || value <= 0) return 0
        return minOf(value, UInt.MAX_VALUE.toDouble()).toLong().toInt()
    }
}
