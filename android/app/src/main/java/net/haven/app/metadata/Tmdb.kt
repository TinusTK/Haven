package net.haven.app.metadata

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import net.haven.app.core.Http
import net.haven.app.core.HttpAnswer
import net.haven.app.core.HttpError
import net.haven.app.titles.JObj
import net.haven.app.titles.MetaDetail
import net.haven.app.titles.MetaPreview
import net.haven.app.titles.ResourceUrl
import net.haven.app.titles.Video
import java.math.BigDecimal
import java.math.RoundingMode

// TMDB (themoviedb.org): extra details for titles with an IMDb id, and
// Haven's catalogue of films and series in the same shapes catalogue
// services use. A TMDB title has the id `tmdb:movie:<n>` or `tmdb:tv:<n>`.
// Needs the user's own TMDB key: none is built in.
//
// This product uses the TMDB API but is not endorsed or certified by TMDB.

/** What can go wrong asking TMDB (or OMDb). */
sealed class MetadataError(message: String) : Exception(message) {
    class Http(val error: HttpError) : MetadataError(error.message ?: "network error")
    class BadKey : MetadataError("TMDB did not accept the API key")
    class Status(val code: Int) : MetadataError("TMDB answered with error $code")
    class BadAnswer : MetadataError("TMDB's answer could not be read")

    override fun equals(other: Any?): Boolean =
        other is MetadataError && other::class == this::class && other.message == message

    override fun hashCode(): Int = message.hashCode()
}

/** What TMDB knows about a title. Empty fields are simply not known. */
data class TmdbDetails(
    val overview: String? = null,
    val runtimeMinutes: Int? = null,
    /** Out of 10. */
    val rating: Double? = null,
    val genres: List<String> = emptyList(),
    val cast: List<String> = emptyList(),
    /** A film's directors, or a series' creators. */
    val directors: List<String> = emptyList(),
    /** Address of a wide picture. */
    val backdrop: String? = null,
    /** Address of the poster. */
    val poster: String? = null,
)

/** The ready-made lists TMDB keeps up to date. */
enum class TmdbList {
    /** What people are watching this week. */
    Trending,
    Popular,
    TopRated,
    /** Films in cinemas now; series with episodes airing now. */
    Now,
    /** Films about to come out (films only). */
    Upcoming,
}

/** A genre as TMDB numbers it. Films and series have separate lists. */
data class Genre(val id: Long, val name: String)

/** A service that carries a title, and the ways it offers it. */
data class Service(val name: String, /** "Subscription", "Free", "Free with ads", "Rent", "Buy". */ val ways: List<String>)

/** Where a title can be watched in one country. The data is JustWatch's,
 *  passed on by TMDB, and must be credited to JustWatch where shown. */
data class Offers(
    /** The country's two-letter code, upper case. */
    val country: String,
    /** TMDB's page for the title in that country. */
    val link: String?,
    val services: List<Service>,
)

/** Talks to TMDB with the user's key: a TMDB "API key" or the longer "API
 *  read access token"; either works (letters, digits and `-_.` are kept). */
class Tmdb(
    key: String,
    private val http: Http = Http.shared,
    val api: String = API,
    val images: String = IMAGES,
) {
    val key: String = key.filter { it.code < 128 && (it.isLetterOrDigit() || it in "-_.") }

    companion object {
        const val API = "https://api.themoviedb.org/3"
        const val IMAGES = "https://image.tmdb.org/t/p"
        const val ANSWER_LIMIT = 4 * 1024 * 1024
        const val CAST_KEPT = 10
        /** TMDB takes at most this many extra parts in one request. */
        const val SEASONS_AT_ONCE = 20

        // Ids

        /** The id Haven gives a TMDB title. */
        fun titleId(series: Boolean, number: Long): String = "tmdb:${if (series) "tv" else "movie"}:$number"

        /** Splits `tmdb:movie:<n>` or `tmdb:tv:<n>` into "is a series" and the
         *  number; null for any other id. */
        fun splitId(id: String): Pair<Boolean, Long>? {
            if (!id.startsWith("tmdb:")) return null
            val rest = id.removePrefix("tmdb:")
            val colon = rest.indexOf(':').takeIf { it >= 0 } ?: return null
            val series = when (rest.substring(0, colon)) {
                "tv" -> true
                "movie" -> false
                else -> return null
            }
            val digits = rest.substring(colon + 1)
            if (digits.isEmpty() || !digits.all { it in '0'..'9' }) return null
            return series to (digits.toLongOrNull() ?: return null)
        }

        /** Whether an id is an IMDb id (`tt` and 1 to 12 digits). */
        fun isImdbId(id: String): Boolean {
            if (!id.startsWith("tt")) return false
            val digits = id.substring(2)
            return digits.isNotEmpty() && digits.length <= 12 && digits.all { it in '0'..'9' }
        }

        // Reading TMDB's answers

        /** Percent-encodes text for use inside a query string. */
        fun escaped(text: String): String = ResourceUrl.encoded(text.trim())

        fun obj(value: JsonElement?): JsonObject? = value as? JsonObject

        fun text(value: JsonElement?, key: String): String? =
            ((obj(value)?.get(key) as? JsonPrimitive)?.takeIf { it.isString }?.content)?.trim()?.takeIf { it.isNotEmpty() }

        /** A whole, non-negative number (serde's as_u64). */
        fun whole(value: JsonElement?): Long? {
            val p = value as? JsonPrimitive ?: return null
            if (p.isString || JObj.isBool(p) || JObj.isFloat(p)) return null
            return p.content.toLongOrNull()?.takeIf { it >= 0 }
        }

        /** Any number (serde's as_f64). */
        fun real(value: JsonElement?): Double? {
            val p = value as? JsonPrimitive ?: return null
            if (p.isString || JObj.isBool(p)) return null
            return p.content.toDoubleOrNull()
        }

        fun list(value: JsonElement?): List<JsonElement> = (value as? JsonArray) ?: emptyList()

        /** "2019-05-04" to "2019". */
        fun year(date: String?): String? = date?.takeIf { it.length >= 4 }?.take(4)

        /** One decimal, rounding a half to even as the Rust and Swift builds do (7.25 is "7.2"). */
        fun oneDecimal(value: Double): String = BigDecimal(value).setScale(1, RoundingMode.HALF_EVEN).toPlainString()

        fun foundId(found: JsonObject, series: Boolean): Long? =
            whole(obj(list(found[if (series) "tv_results" else "movie_results"]).firstOrNull())?.get("id"))

        /** A list answer as posters. Entries with no id or name are left out. */
        fun readPage(found: JsonObject, series: Boolean, images: String): List<MetaPreview> =
            list(found["results"]).mapNotNull { entry ->
                val id = whole(obj(entry)?.get("id")) ?: return@mapNotNull null
                val name = text(entry, "title") ?: text(entry, "name") ?: return@mapNotNull null
                MetaPreview(
                    id = titleId(series, id), kind = if (series) "series" else "movie", name = name,
                    poster = text(entry, "poster_path")?.let { "$images/w342$it" },
                    releaseInfo = year(text(entry, "release_date") ?: text(entry, "first_air_date")),
                    rating = real(obj(entry)?.get("vote_average"))?.takeIf { it > 0 }?.let { oneDecimal(it) },
                )
            }

        fun readGenres(found: JsonObject): List<Genre> = list(found["genres"]).mapNotNull { item ->
            val id = whole(obj(item)?.get("id")) ?: return@mapNotNull null
            val name = text(item, "name") ?: return@mapNotNull null
            Genre(id, name)
        }

        fun readOffers(answer: JsonObject, countries: List<String>): Offers? {
            val results = obj(answer["results"]) ?: return null
            val ways = listOf("flatrate" to "Subscription", "free" to "Free", "ads" to "Free with ads", "rent" to "Rent", "buy" to "Buy")
            for (wanted in countries) {
                val country = wanted.trim().uppercase()
                val entry = obj(results[country]) ?: continue
                val services = mutableListOf<Service>()
                for ((key, way) in ways) {
                    for (name in list(entry[key]).mapNotNull { text(it, "provider_name") }) {
                        val at = services.indexOfFirst { it.name == name }
                        if (at >= 0) services[at] = services[at].copy(ways = services[at].ways + way) else services += Service(name, listOf(way))
                    }
                }
                if (services.isNotEmpty()) return Offers(country, text(entry, "link"), services)
            }
            return null
        }

        /** The numbers of a series' seasons that have episodes, specials included. */
        fun seasonNumbers(record: JsonObject): List<Int> = list(record["seasons"]).mapNotNull { item ->
            val season = obj(item)
            if ((whole(season?.get("episode_count")) ?: 0) <= 0) return@mapNotNull null
            whole(season?.get("season_number"))?.toInt()
        }

        fun readDetails(record: JsonObject, images: String = IMAGES): TmdbDetails {
            fun names(value: JsonElement?) = list(value).mapNotNull { rawName(it) }
            val credits = obj(record["credits"])
            var directors = list(credits?.get("crew")).mapNotNull { member ->
                if (rawText(member, "job") != "Director") null else rawText(member, "name")
            }
            if (directors.isEmpty()) directors = names(record["created_by"])
            val runtime = whole(record["runtime"]) ?: whole(list(record["episode_run_time"]).firstOrNull())
            return TmdbDetails(
                overview = text(record, "overview"),
                runtimeMinutes = runtime?.takeIf { it > 0 }?.toInt(),
                rating = real(record["vote_average"])?.takeIf { it > 0 },
                genres = names(record["genres"]),
                cast = names(credits?.get("cast")).take(CAST_KEPT),
                directors = directors,
                backdrop = text(record, "backdrop_path")?.let { "$images/w1280$it" },
                poster = text(record, "poster_path")?.let { "$images/w500$it" },
            )
        }

        /** A name as given (not trimmed), as the Swift build reads names. */
        private fun rawName(value: JsonElement?): String? = rawText(value, "name")

        private fun rawText(value: JsonElement?, key: String): String? =
            (obj(value)?.get(key) as? JsonPrimitive)?.takeIf { it.isString }?.content

        /** A title's own record as a page of details (episodes come separately). */
        fun readTitle(record: JsonObject, series: Boolean, number: Long, images: String): MetaDetail? {
            val name = text(record, "title") ?: text(record, "name") ?: return null
            val details = readDetails(record, images)
            val released = text(record, "release_date") ?: text(record, "first_air_date")
            return MetaDetail(
                id = titleId(series, number), kind = if (series) "series" else "movie", name = name,
                poster = details.poster, background = details.backdrop, description = details.overview,
                releaseInfo = year(released), released = released,
                runtime = details.runtimeMinutes?.let { "$it min" }, rating = details.rating?.let { oneDecimal(it) },
                genres = details.genres, cast = details.cast, director = details.directors,
                imdbId = (text(record, "imdb_id") ?: text(record["external_ids"], "imdb_id"))?.takeIf { isImdbId(it) },
                tmdbId = number.toString(),
            )
        }

        /** The episodes of one season from an answer that carries `season/<n>`.
         *  An episode's id is `<imdb id>:<season>:<episode>`, the form other
         *  sources know, or `tmdb:tv:<number>:<season>:<episode>` without one. */
        fun readSeason(answer: JsonObject, meta: MetaDetail, number: Long, season: Int, images: String = IMAGES): List<Video> {
            val stem = meta.imdbId ?: titleId(true, number)
            return list(obj(answer["season/$season"])?.get("episodes")).mapNotNull { item ->
                val episode = whole(obj(item)?.get("episode_number")) ?: return@mapNotNull null
                Video(
                    id = "$stem:$season:$episode", title = text(item, "name"), season = season, episode = episode.toInt(),
                    released = text(item, "air_date"), thumbnail = text(item, "still_path")?.let { "$images/w300$it" },
                )
            }
        }

        private fun kind(series: Boolean) = if (series) "tv" else "movie"
    }

    // Asking

    private suspend fun get(path: String): JsonObject {
        // The long token is sent as a header, the short key in the address.
        var url = api + path
        val headers = mutableListOf<Pair<String, String>>()
        if (key.contains(".")) headers += "Authorization" to "Bearer $key" else url += (if (path.contains("?")) "&" else "?") + "api_key=$key"
        val answer: HttpAnswer = try {
            http.send(url, headers = headers, limit = ANSWER_LIMIT)
        } catch (e: HttpError) {
            throw MetadataError.Http(e)
        }
        return when (answer.status) {
            in 200..299 -> runCatching { Json.parseToJsonElement(answer.text) as JsonObject }.getOrNull() ?: throw MetadataError.BadAnswer()
            401 -> throw MetadataError.BadKey()
            else -> throw MetadataError.Status(answer.status)
        }
    }

    /** One page (20 titles) of a ready-made list; pages count from 1. */
    suspend fun list(list: TmdbList, series: Boolean, page: Int): List<MetaPreview> {
        val kind = kind(series)
        val path = when (list) {
            TmdbList.Trending -> "/trending/$kind/week"
            TmdbList.Popular -> "/$kind/popular"
            TmdbList.TopRated -> "/$kind/top_rated"
            TmdbList.Now -> if (series) "/tv/on_the_air" else "/movie/now_playing"
            TmdbList.Upcoming -> "/movie/upcoming"
        }
        return readPage(get("$path?page=${maxOf(page, 1)}"), series, images)
    }

    /** One page of every film or series, most popular first, narrowed to a genre if given. */
    suspend fun discover(series: Boolean, genre: Long?, page: Int): List<MetaPreview> {
        var path = "/discover/${kind(series)}?sort_by=popularity.desc&page=${maxOf(page, 1)}"
        if (genre != null) path += "&with_genres=$genre"
        return readPage(get(path), series, images)
    }

    suspend fun genres(series: Boolean): List<Genre> = readGenres(get("/genre/${kind(series)}/list"))

    /** Films or series whose name matches `words`. */
    suspend fun search(series: Boolean, words: String): List<MetaPreview> =
        readPage(get("/search/${kind(series)}?query=${escaped(words)}"), series, images)

    /** The TMDB number of the title with that IMDb id, if TMDB knows it. */
    suspend fun numberOf(series: Boolean, imdbId: String): Long? {
        if (!isImdbId(imdbId)) return null
        return foundId(get("/find/$imdbId?external_source=imdb_id"), series)
    }

    /** The services that carry a title, in the first of `countries` that has any. */
    suspend fun offers(series: Boolean, number: Long, countries: List<String>): Offers? =
        readOffers(get("/${kind(series)}/$number/watch/providers"), countries)

    /** Everything about one title; for a series, with all its episodes. */
    suspend fun title(series: Boolean, number: Long): MetaDetail {
        val record = get("/${kind(series)}/$number?append_to_response=credits,external_ids")
        var meta = readTitle(record, series, number, images) ?: throw MetadataError.BadAnswer()
        if (series) {
            val videos = mutableListOf<Video>()
            for (group in seasonNumbers(record).chunked(SEASONS_AT_ONCE)) {
                val parts = group.joinToString(",") { "season/$it" }
                // A season that cannot be fetched is left out, not fatal.
                val answer = runCatching { get("/tv/$number?append_to_response=$parts") }.getOrNull() ?: continue
                for (season in group) videos += readSeason(answer, meta, number, season, images)
            }
            meta = meta.copy(videos = meta.videos + videos)
        }
        return meta
    }

    /** Looks a title up by its IMDb id: TMDB's details for filling in what the
     *  title's own source lacks. Null when TMDB does not know it. */
    suspend fun details(series: Boolean, imdbId: String): TmdbDetails? {
        val number = numberOf(series, imdbId) ?: return null
        return readDetails(get("/${kind(series)}/$number?append_to_response=credits"), images)
    }
}
