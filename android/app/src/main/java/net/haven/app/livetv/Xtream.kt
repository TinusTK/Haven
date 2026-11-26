package net.haven.app.livetv

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import net.haven.app.servers.SJ
import net.haven.app.titles.ResourceUrl

// Xtream Codes: a provider's account (server address, user name and
// password) instead of a playlist address. The account's live channels are
// asked for through its `player_api.php` and turned into the same playlist
// text an M3U source gives. Its films and series are offered as places to
// watch a title.

sealed class XtreamError(message: String) : Exception(message) {
    class BadAnswer : XtreamError("the server's answer was not an Xtream Codes answer")
    class Refused : XtreamError("the user name or password was not accepted")
    class NotActive(val status: String) : XtreamError("the account is $status")

    override fun equals(other: Any?): Boolean = other is XtreamError && other::class == this::class && other.message == message
    override fun hashCode(): Int = message.hashCode()
}

/** An Xtream Codes account. Never prints its password. */
class XtreamAccount private constructor(
    /** Scheme, host and port, without a trailing slash. */
    val server: String,
    val username: String,
    val password: String,
) {
    companion object {
        /** Tidies what the user typed. The server may be given with or without
         *  `http://`, and with a path: only the scheme, host and port are kept.
         *  Null if any of the three parts is missing or the server is not a web address. */
        fun of(server: String, username: String, password: String): XtreamAccount? {
            val typed = server.trim()
            val user = username.trim()
            val pass = password.trim()
            val mark = typed.indexOf("://")
            val scheme = if (mark >= 0) typed.substring(0, mark).lowercase() else "http"
            val rest = if (mark >= 0) typed.substring(mark + 3) else typed
            val full = rest.split('/', '?', '#').first()
            // "user:password@" in front of the host is not part of it.
            val host = full.substringAfterLast('@')
            if (host.isEmpty() || host.any { it.isWhitespace() } || (scheme != "http" && scheme != "https") || user.isEmpty() || pass.isEmpty()) {
                return null
            }
            return XtreamAccount("$scheme://$host", user, pass)
        }

        /** Reads an account back from the address it was stored under; null for
         *  any other address (an M3U playlist's). */
        fun fromSourceUrl(url: String): XtreamAccount? {
            val mark = url.indexOf("/player_api.php?").takeIf { it >= 0 } ?: return null
            val server = url.substring(0, mark)
            val pairs = url.substring(mark + "/player_api.php?".length).split('&')
            if (pairs.any { it.startsWith("action=") }) return null
            fun value(name: String): String? = pairs.firstNotNullOfOrNull { pair ->
                val eq = pair.indexOf('=').takeIf { it >= 0 } ?: return@firstNotNullOfOrNull null
                if (pair.substring(0, eq) == name) decoded(pair.substring(eq + 1)) else null
            }
            return of(server, value("username") ?: return null, value("password") ?: return null)
        }

        private fun encoded(text: String) = ResourceUrl.encoded(text)

        /** Percent-decoding, with `+` as a space. */
        private fun decoded(part: String): String {
            val bytes = part.toByteArray()
            val out = java.io.ByteArrayOutputStream()
            var at = 0
            while (at < bytes.size) {
                val hex = if (bytes[at] == '%'.code.toByte() && at + 2 < bytes.size) String(bytes, at + 1, 2).toIntOrNull(16) else null
                if (hex != null) {
                    out.write(hex)
                    at += 3
                } else {
                    out.write(if (bytes[at] == '+'.code.toByte()) ' '.code else bytes[at].toInt())
                    at += 1
                }
            }
            return out.toString(Charsets.UTF_8)
        }
    }

    /** The address to ask: with no action, about the account itself. */
    fun api(action: String? = null): String {
        var url = "$server/player_api.php?username=${encoded(username)}&password=${encoded(password)}"
        if (action != null) url += "&action=${encoded(action)}"
        return url
    }

    /** The address Haven stores the account under, as a source. */
    val sourceUrl: String get() = api()

    /** How the account is shown in lists: the user name and the host, never the password. */
    val label: String get() = "$username at ${M3U.sourceName(server)}"

    /** Where one of the account's films plays from. */
    fun film(id: String, extension: String) = file("movie", id, extension)

    /** Where one episode of one of the account's series plays from. */
    fun episode(id: String, extension: String) = file("series", id, extension)

    private fun file(kind: String, id: String, extension: String): String {
        // Only letters and digits make a file ending; anything else is left off.
        val ending = extension.filter { it.code < 128 && it.isLetterOrDigit() }
        return "$server/$kind/${encoded(username)}/${encoded(password)}/${encoded(id)}.${ending.ifEmpty { "mp4" }}"
    }

    /** Where one live channel plays from. */
    fun stream(id: String, format: String) = "$server/live/${encoded(username)}/${encoded(password)}/${encoded(id)}.$format"

    override fun equals(other: Any?): Boolean =
        other is XtreamAccount && other.server == server && other.username == username && other.password == password

    override fun hashCode(): Int = listOf(server, username, password).hashCode()

    override fun toString(): String = "XtreamAccount(server=$server, username=$username, ..)"
}

/** What the server says about the account when signing in. */
data class XtreamInfo(
    /** When the account runs out, in seconds since 1970; null if never. */
    val expires: Long? = null,
    /** The stream formats the account may use ("ts", "m3u8"…). */
    val formats: List<String> = emptyList(),
)

/** A film or a series in an account's library, as its list names it. */
data class OnDemand(
    /** The name as the provider wrote it ("EN - A Film (2021) 4K"). */
    val name: String,
    /** A film's stream id, or a series' id. */
    val id: String,
    /** A film's file ending; empty for a series. */
    val ext: String,
    val year: String? = null,
    /** Its number at TMDB, where the list gives one. */
    val tmdb: String? = null,
)

object Xtream {
    private fun parse(json: String): JsonElement? = runCatching { Json.parseToJsonElement(json) }.getOrNull()
        ?.takeUnless { it is JsonPrimitive && !it.isString && it.content.toDoubleOrNull() == null && it.content !in listOf("true", "false", "null") }

    /** Reads the answer to signing in (the API asked with no action). */
    fun signIn(json: String): XtreamInfo {
        val user = SJ.obj(parse(json), "user_info") ?: throw XtreamError.BadAnswer()
        if (SJ.text(user, "auth") != "1") throw XtreamError.Refused()
        val status = SJ.text(user, "status")
        if (status != null && !status.equals("active", ignoreCase = true)) throw XtreamError.NotActive(status.lowercase())
        return XtreamInfo(
            expires = SJ.text(user, "exp_date")?.toLongOrNull(),
            formats = SJ.list(user, "allowed_output_formats").mapNotNull { (it as? JsonPrimitive)?.takeIf { p -> p.isString }?.content?.lowercase() },
        )
    }

    /** The account's live channels, from its answers to `get_live_categories`
     *  and `get_live_streams`. Channels without an id or a name are left out; a
     *  missing or unreadable category list only costs the group names. */
    fun channels(account: XtreamAccount, info: XtreamInfo, categories: String, streams: String): List<Channel> {
        val list = parse(streams) as? JsonArray ?: throw XtreamError.BadAnswer()
        val names = mutableMapOf<String, String>()
        for (category in (parse(categories) as? JsonArray) ?: emptyList()) {
            val id = SJ.text(category, "category_id") ?: continue
            val name = SJ.text(category, "category_name") ?: continue
            names.putIfAbsent(id, name)
        }
        // The plain transport stream unless the account is only let have HLS.
        val onlyHls = info.formats.isNotEmpty() && "ts" !in info.formats && "m3u8" in info.formats
        val format = if (onlyHls) "m3u8" else "ts"
        return list.mapNotNull { stream ->
            val id = SJ.text(stream, "stream_id") ?: return@mapNotNull null
            val name = SJ.text(stream, "name") ?: return@mapNotNull null
            Channel(
                name = name, address = account.stream(id, format),
                group = SJ.text(stream, "category_id")?.let { names[it] },
                logo = SJ.text(stream, "stream_icon"), guideId = SJ.text(stream, "epg_channel_id"),
                radio = SJ.text(stream, "stream_type")?.contains("radio") ?: false,
            )
        }
    }

    private fun onDemand(json: String, idKey: String): List<OnDemand> {
        val listed = parse(json) as? JsonArray ?: throw XtreamError.BadAnswer()
        fun year(text: String?) = text?.take(4)?.takeIf { it.length == 4 && it.all { c -> c in '0'..'9' } }
        return listed.mapNotNull { entry ->
            val name = SJ.text(entry, "name") ?: return@mapNotNull null
            val id = SJ.text(entry, idKey) ?: return@mapNotNull null
            OnDemand(
                name = name, id = id, ext = SJ.text(entry, "container_extension") ?: "",
                year = year(SJ.text(entry, "year")) ?: year(SJ.text(entry, "releaseDate")) ?: year(SJ.text(entry, "release_date")),
                tmdb = (SJ.text(entry, "tmdb") ?: SJ.text(entry, "tmdb_id"))?.takeUnless { it == "0" },
            )
        }
    }

    /** The account's films, from its answer to `get_vod_streams`. */
    fun films(json: String) = onDemand(json, "stream_id")

    /** The account's series, from its answer to `get_series`. */
    fun series(json: String) = onDemand(json, "series_id")

    private fun isYear(word: String) = word.length == 4 && word.all { it in '0'..'9' } && (word.startsWith("19") || word.startsWith("20"))

    /** A provider's name for a title, down to what tells titles apart: no
     *  country tag in front ("EN - "), nothing from a bracket on, no year or
     *  picture-quality words at the end; letters and digits only. With it
     *  comes a year found in the name, if any. */
    fun plainTitle(name: String): Pair<String, String?> {
        val parts = name.split('(', '[')
        var year: String? = parts.drop(1).firstNotNullOfOrNull { part -> part.take(4).takeIf { isYear(it) } }
        var head = parts.first().trim()
        // "EN - ", "UK: ", "4K| " in front: a short tag in capitals, then a mark.
        val splits = listOfNotNull(
            head.indexOf(" - ").takeIf { it >= 0 }?.let { head.substring(0, it) to head.substring(it + 3) },
            head.indexOf('|').takeIf { it >= 0 }?.let { head.substring(0, it) to head.substring(it + 1) },
            head.indexOf(':').takeIf { it >= 0 }?.let { head.substring(0, it) to head.substring(it + 1) },
        )
        val tagged = splits.firstOrNull { (tag, rest) ->
            val t = tag.trim()
            t.length in 1..5 &&
                t.all { (it.code < 128 && it.isUpperCase()) || (it.code < 128 && it.isDigit()) || it == '+' } &&
                t.any { it.code < 128 && it.isUpperCase() } &&
                rest.trim().isNotEmpty()
        }
        if (tagged != null) head = tagged.second.trim()
        val words = head.split(Regex("\\s+")).filter { it.isNotEmpty() }.toMutableList()
        val quality = setOf("4k", "uhd", "fhd", "hd", "sd", "hdr", "hevc", "multi", "1080p", "720p", "2160p", "480p")
        while (words.size > 1) {
            val last = words.last().lowercase()
            if (isYear(last)) {
                if (year == null) year = last
            } else if (last !in quality) {
                break
            }
            words.removeAt(words.size - 1)
        }
        val plain = words.joinToString("").filter { it.isLetterOrDigit() }.lowercase()
        return plain to year
    }

    /** The entries of a list that are the wanted title: by TMDB number where
     *  both sides have one, else by name, and by year where both sides have
     *  one. All repeats (other qualities or languages) are returned. */
    fun matches(listed: List<OnDemand>, name: String, year: String?, tmdb: String?): List<OnDemand> {
        val wanted = plainTitle(name).first
        val wantedYear = year?.take(4)
        return listed.filter { entry ->
            if (tmdb != null && entry.tmdb != null) return@filter tmdb == entry.tmdb
            val (plain, yearInName) = plainTitle(entry.name)
            if (wanted.isEmpty() || plain != wanted) return@filter false
            val other = entry.year ?: yearInName
            if (wantedYear != null && other != null) wantedYear == other else true
        }
    }

    /** One episode from the account's answer to `get_series_info`: its id and file ending. */
    fun episode(json: String, season: Int, episode: Int): Pair<String, String>? {
        val episodes = (parse(json) as? JsonObject)?.get("episodes") ?: return null
        // Seasons come as an object keyed by number, or as a list of lists.
        val seasons: List<JsonElement> = when (episodes) {
            is JsonObject -> episodes.keys.sorted().mapNotNull { episodes[it] }
            is JsonArray -> episodes
            else -> return null
        }
        for (entry in seasons.mapNotNull { it as? JsonArray }.flatten()) {
            fun number(key: String) = SJ.text(entry, key)?.toUIntOrNull()?.toInt()
            if (number("season") != season || number("episode_num") != episode) continue
            val id = SJ.text(entry, "id") ?: continue
            return id to (SJ.text(entry, "container_extension") ?: "")
        }
        return null
    }
}
