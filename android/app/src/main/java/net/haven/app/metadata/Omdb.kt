package net.haven.app.metadata

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import net.haven.app.core.Http
import net.haven.app.core.HttpAnswer
import net.haven.app.core.HttpError

/** OMDb (omdbapi.com): the scores TMDB and Cinemeta do not have (Rotten
 *  Tomatoes, Metacritic) and the age rating, for a title with an IMDb id.
 *  Needs an OMDb key (one is built into Haven's builds; the free key allows
 *  1,000 requests a day, so answers are kept for 7 days). */
object Omdb {
    const val API = "https://www.omdbapi.com/"

    /** What OMDb knows about a title. Saved with the Rust build's field names. */
    @Serializable
    data class Scores(
        /** "8.4" */
        val imdb: String? = null,
        /** "94%" */
        @SerialName("rotten_tomatoes") val rottenTomatoes: String? = null,
        /** "78" (out of 100) */
        val metacritic: String? = null,
        /** "R", "PG-13", "TV-MA"… */
        val rated: String? = null,
        /** "173 min" */
        val runtime: String? = null,
    )

    /** Text that is there and not "N/A". */
    private fun known(value: JsonElement?): String? =
        (value as? JsonPrimitive)?.takeIf { it.isString }?.content?.trim()?.takeIf { it.isNotEmpty() && it != "N/A" }

    fun readScores(answer: JsonObject): Scores? {
        if (known(answer["Response"]) == "False") return null
        val rottenTomatoes = Tmdb.list(answer["Ratings"]).firstNotNullOfOrNull { item ->
            val rating = item as? JsonObject ?: return@firstNotNullOfOrNull null
            if (known(rating["Source"]) != "Rotten Tomatoes") null else known(rating["Value"])
        }
        val rated = known(answer["Rated"])?.takeUnless { it == "Not Rated" || it == "Unrated" }
        return Scores(known(answer["imdbRating"]), rottenTomatoes, known(answer["Metascore"]), rated, known(answer["Runtime"]))
    }

    /** The scores for the title with IMDb id `imdb`; null when OMDb does not know it. */
    suspend fun scores(key: String, imdb: String, http: Http = Http.shared): Scores? {
        if (!Tmdb.isImdbId(imdb)) return null
        val clean = key.filter { it.code < 128 && it.isLetterOrDigit() }
        val answer: HttpAnswer = try {
            http.send("$API?i=$imdb&apikey=$clean", limit = Tmdb.ANSWER_LIMIT)
        } catch (e: HttpError) {
            throw MetadataError.Http(e)
        }
        return when (answer.status) {
            in 200..299 -> readScores(runCatching { Json.parseToJsonElement(answer.text) as JsonObject }.getOrNull() ?: throw MetadataError.BadAnswer())
            401 -> throw MetadataError.BadKey()
            else -> throw MetadataError.Status(answer.status)
        }
    }
}
