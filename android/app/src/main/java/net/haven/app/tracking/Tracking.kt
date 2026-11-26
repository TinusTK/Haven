package net.haven.app.tracking

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonObject
import kotlinx.serialization.json.addJsonObject
import kotlinx.serialization.json.putJsonArray
import net.haven.app.core.HttpError
import net.haven.app.core.Log
import net.haven.app.core.Store
import net.haven.app.metadata.Tmdb
import net.haven.app.titles.JObj
import java.time.OffsetDateTime

// Tracking what was watched with Trakt and Simkl: signing in with a short
// code, marking films and episodes watched, and keeping the Watchlist in
// step. Both services know titles by their IMDb id, so only titles that
// carry one can be tracked. Each service needs Haven's app key (a client id;
// older Trakt apps also a client secret).

sealed class TrackingError(message: String) : Exception(message) {
    class Http(val error: HttpError) : TrackingError(error.message ?: "network error")
    class NoKey(val service: String) : TrackingError("no $service key is set in Settings")
    class Refused(val service: String) : TrackingError("$service did not accept the key or the sign-in")
    class Status(val code: Int) : TrackingError("the service answered with error $code")
    class BadAnswer : TrackingError("the service's answer could not be read")
    class Expired : TrackingError("the code ran out before it was approved")
    class Network(why: String) : TrackingError("could not reach the service: $why")

    override fun equals(other: Any?): Boolean = other is TrackingError && other::class == this::class && other.message == message
    override fun hashCode(): Int = message.hashCode()
}

@Serializable
enum class TrackingService(val label: String) {
    @SerialName("trakt") Trakt("Trakt"),
    @SerialName("simkl") Simkl("Simkl"),
}

/** A signed-in account, as kept between runs (kv `tracking_accounts`, the
 *  Rust build's shape). Never prints its tokens. */
@Serializable
data class TrackedAccount(
    val service: TrackingService,
    val token: String,
    /** Exchanges for a new token when this one runs out. */
    val refresh: String? = null,
    /** When the token runs out, in seconds since 1970, if it does. */
    @SerialName("expires_at") val expiresAt: Long? = null,
) {
    override fun toString(): String = "TrackedAccount(service=${service.label}, ..)"

    companion object {
        const val KEY = "tracking_accounts"

        fun load(store: Store): List<TrackedAccount> = runCatching { store.get<List<TrackedAccount>>(KEY) }.getOrNull() ?: emptyList()

        fun save(accounts: List<TrackedAccount>, store: Store) {
            runCatching { store.put(KEY, accounts) }.onFailure { Log.note("could not save the tracking sign-ins: ${it.message}") }
        }
    }
}

/** A sign-in in progress: the user approves `userCode` at `link`. */
data class SignInCode(
    val service: TrackingService,
    val userCode: String,
    val link: String,
    /** What Haven asks with while waiting. */
    val deviceCode: String,
    /** Seconds between asks. */
    val interval: Int,
    /** Seconds until the code runs out. */
    val expiresIn: Int,
)

/** A film, a series, or one episode, by IMDb id. */
data class Subject(val imdb: String, val series: Boolean, /** Season and episode, for one episode. */ val episode: Pair<Int, Int>? = null)

/** A title on a service's Watchlist. */
data class Listed(val imdb: String, val series: Boolean, val title: String, val year: String)

/** A film or episode the service says was watched. */
data class Watched(
    val imdb: String,
    val title: String,
    /** Season and episode for an episode; null for a film. */
    val episode: Pair<Int, Int>? = null,
    /** When, in seconds since 1970 (0 if not given). */
    val at: Long = 0,
)

object Tracking {
    /** What to track for one of Haven's titles: null if it has no IMDb id.
     *  An episode must be named `<imdb id>:<season>:<episode>`. */
    fun subject(kind: String, titleId: String, videoId: String?): Subject? {
        if (!Tmdb.isImdbId(titleId)) return null
        val series = kind == "series"
        var episode: Pair<Int, Int>? = null
        if (series && videoId != null) {
            val parts = videoId.split(':')
            val s = parts.getOrNull(1)?.toUIntOrNull()
            val e = parts.getOrNull(2)?.toUIntOrNull()
            if (parts.size == 3 && parts[0] == titleId && s != null && e != null) episode = s.toInt() to e.toInt()
        }
        return Subject(titleId, series, episode)
    }

    /** The JSON both services take to name a subject. */
    fun body(subject: Subject, simklList: String? = null): JsonObject {
        val entry = buildJsonObject {
            putJsonObject("ids") { put("imdb", subject.imdb) }
            if (simklList != null) put("to", simklList)
            subject.episode?.let { (season, number) ->
                putJsonArray("seasons") {
                    addJsonObject {
                        put("number", season)
                        putJsonArray("episodes") { addJsonObject { put("number", number) } }
                    }
                }
            }
        }
        return buildJsonObject { put(if (subject.series) "shows" else "movies", buildJsonArray { add(entry) }) }
    }

    /** An RFC 3339 moment in seconds since 1970; 0 if not readable. */
    fun moment(value: JsonElement?): Long {
        val text = (value as? JsonPrimitive)?.takeIf { it.isString }?.content ?: return 0
        return runCatching { OffsetDateTime.parse(text).toEpochSecond() }.getOrDefault(0)
    }

    private fun obj(value: JsonElement?) = value as? JsonObject

    private fun str(value: JsonElement?) = (value as? JsonPrimitive)?.takeIf { it.isString }?.content

    private fun imdb(title: JsonElement?): String? = str(obj(obj(title)?.get("ids"))?.get("imdb"))?.takeIf { Tmdb.isImdbId(it) }

    private fun entries(answer: JsonElement?, series: Boolean): List<JsonElement> =
        (answer as? JsonArray) ?: (obj(answer)?.get(if (series) "shows" else "movies") as? JsonArray) ?: emptyList()

    /** Trakt's `/sync/watched/…` answer, or Simkl's `/sync/all-items/…` one
     *  (wrapped in {"movies": […]} or {"shows": […]}, an episode's time in `watched_at`). */
    fun readWatched(answer: JsonElement?, series: Boolean): List<Watched> {
        val key = if (series) "show" else "movie"
        val found = mutableListOf<Watched>()
        for (item in entries(answer, series)) {
            val entry = obj(item) ?: continue
            val title = entry[key] ?: continue
            val id = imdb(title) ?: continue
            val name = str(obj(title)?.get("title")) ?: ""
            if (!series) {
                found += Watched(id, name, null, moment(entry["last_watched_at"]))
                continue
            }
            for (season in Tmdb.list(entry["seasons"])) {
                val number = Tmdb.whole(obj(season)?.get("number")) ?: continue
                for (episode in Tmdb.list(obj(season)?.get("episodes"))) {
                    val episodeNumber = Tmdb.whole(obj(episode)?.get("number")) ?: continue
                    val at = obj(episode)?.let { it["last_watched_at"] ?: it["watched_at"] }
                    found += Watched(id, name, number.toInt() to episodeNumber.toInt(), moment(at))
                }
            }
        }
        return found
    }

    /** A Watchlist answer: Trakt answers with a list; Simkl wraps it in
     *  {"movies": […]} or {"shows": […]}. */
    fun readListed(answer: JsonElement?, series: Boolean): List<Listed> {
        val key = if (series) "show" else "movie"
        return entries(answer, series).mapNotNull { entry ->
            val title = obj(obj(entry)?.get(key)) ?: return@mapNotNull null
            val id = imdb(title) ?: return@mapNotNull null
            val name = str(title["title"]) ?: return@mapNotNull null
            val year = (title["year"] as? JsonPrimitive)?.let { p -> if (p.isString) p.content else if (JObj.isBool(p)) "" else JObj.text(p) ?: "" } ?: ""
            Listed(id, series, name, year)
        }
    }

    /** A whole number (serde's as_i64). */
    private fun integer(value: JsonElement?): Long? {
        val p = value as? JsonPrimitive ?: return null
        if (p.isString || JObj.isBool(p) || JObj.isFloat(p)) return null
        return p.content.toLongOrNull()
    }

    /** A Simkl sign-in from a token answer; `refresh` is kept when the answer leaves it out. */
    fun readSimklToken(answer: JsonElement?, now: Long, refresh: String?): TrackedAccount? {
        val o = obj(answer) ?: return null
        val token = str(o["access_token"]) ?: return null
        return TrackedAccount(TrackingService.Simkl, token, str(o["refresh_token"]) ?: refresh, integer(o["expires_in"])?.let { now + it })
    }

    fun readTraktToken(answer: JsonElement?, now: Long): TrackedAccount? {
        val o = obj(answer) ?: return null
        val token = str(o["access_token"]) ?: return null
        return TrackedAccount(
            TrackingService.Trakt, token, str(o["refresh_token"]),
            integer(o["expires_in"])?.let { (integer(o["created_at"]) ?: now) + it },
        )
    }
}
