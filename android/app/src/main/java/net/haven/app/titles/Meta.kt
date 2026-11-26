package net.haven.app.titles

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonArray
import net.haven.app.core.HttpError

/** What can go wrong reading titles from a catalogue service. */
sealed class TitlesError(message: String) : Exception(message) {
    class InvalidAddress(val address: String) : TitlesError("\"$address\" is not a web address")
    class Http(val error: HttpError) : TitlesError(error.message ?: "network error")
    class NotText : TitlesError("the answer was not text")
    class BadResponse(val why: String) : TitlesError("the answer could not be read: $why")
    class UnknownTitle : TitlesError("this title is not known there")

    override fun equals(other: Any?): Boolean =
        other is TitlesError && other::class == this::class && other.message == message

    override fun hashCode(): Int = message.hashCode()
}

/** The short form of a title that catalogues list: enough for a poster. */
@Serializable
data class MetaPreview(
    val id: String,
    /** "movie", "series", … */
    val kind: String,
    val name: String,
    val poster: String? = null,
    /** Usually a year or a year range; some services send a number. */
    val releaseInfo: String? = null,
    /** Out of 10, as text ("8.4"), when the catalogue gives one. */
    val rating: String? = null,
) {
    companion object {
        fun from(o: JObj) = MetaPreview(
            id = o.string("id"), kind = o.string("type"), name = o.string("name"),
            poster = o.optionalString("poster"), releaseInfo = o.textOrNumber("releaseInfo"),
            rating = o.textOrNumber("imdbRating"),
        )
    }
}

/** One episode. Its `id` is what streams are asked for. */
@Serializable
data class Video(
    val id: String,
    /** `title` now, `name` in older services. */
    val title: String? = null,
    val season: Int? = null,
    /** `episode` now, `number` in older services. */
    val episode: Int? = null,
    /** When it first aired, as an ISO date. */
    val released: String? = null,
    /** A still from the episode (a web address), if the source has one. */
    val thumbnail: String? = null,
) {
    companion object {
        fun from(o: JObj) = Video(
            id = o.string("id"),
            // Both spellings together: the current one counts (even if null).
            title = if (o.present("title")) o.textOrNumber("title") else o.textOrNumber("name"),
            season = o.number("season"),
            episode = if (o.present("episode")) o.number("episode") else o.number("number"),
            released = o.textOrNumber("released"),
            thumbnail = o.textOrNumber("thumbnail"),
        )
    }
}

/** Everything a service knows about one title. */
@Serializable
data class MetaDetail(
    val id: String,
    val kind: String,
    val name: String,
    val poster: String? = null,
    /** A wide picture for the top of the page. */
    val background: String? = null,
    /** The title's name drawn as a picture, when there is one. */
    val logo: String? = null,
    val description: String? = null,
    val releaseInfo: String? = null,
    val runtime: String? = null,
    val rating: String? = null,
    /** `genres` now, `genre` in older services. */
    val genres: List<String> = emptyList(),
    val cast: List<String> = emptyList(),
    val director: List<String> = emptyList(),
    /** When the title itself first came out, as an ISO date. */
    val released: String? = null,
    /** Episodes of a series (or the videos of a channel). Empty for films. */
    val videos: List<Video> = emptyList(),
    /** The title's IMDb id (`tt…`) when its own id is something else. */
    val imdbId: String? = null,
    /** The title's number at TMDB, if known. */
    val tmdbId: String? = null,
) {
    /** The seasons that have episodes, in order, specials (season 0) last. */
    val seasons: List<Int>
        get() = videos.mapNotNull { it.season }.distinct().sortedWith(compareBy({ if (it == 0) 1 else 0 }, { it }))

    /** The episodes of one season, in episode order (ones with no number first). */
    fun episodes(season: Int): List<Video> =
        videos.withIndex().filter { it.value.season == season }
            .sortedWith(compareBy({ it.value.episode ?: -1 }, { it.index }))
            .map { it.value }

    companion object {
        fun from(o: JObj): MetaDetail {
            val videos = when (val list = o["videos"]) {
                null -> emptyList()
                is JsonArray -> list.mapNotNull { item ->
                    // Entries that are not objects or have no text id are left out.
                    val video = JObj.of(item) ?: return@mapNotNull null
                    if (runCatching { video.string("id") }.isFailure) null else Video.from(video)
                }
                else -> throw JObj.Wrong("videos")
            }
            return MetaDetail(
                id = o.string("id"), kind = o.string("type"), name = o.string("name"),
                poster = o.optionalString("poster"), background = o.optionalString("background"),
                logo = o.optionalString("logo"), description = o.optionalString("description"),
                releaseInfo = o.textOrNumber("releaseInfo"), runtime = o.textOrNumber("runtime"),
                rating = o.textOrNumber("imdbRating"),
                // Both spellings together: the current one counts.
                genres = if (o.present("genres")) o.texts("genres") else o.texts("genre"),
                cast = o.texts("cast"), director = o.texts("director"),
                released = o.textOrNumber("released"), videos = videos,
                imdbId = o.textOrNumber("imdb_id"), tmdbId = o.textOrNumber("moviedb_id"),
            )
        }
    }
}

/** Reading what catalogue services send, and telling titles apart. */
object Titles {
    /** Whether a name reads like a file's name ("Film (2024) [1080p]"). */
    fun isReleaseName(name: String): Boolean = name.contains("[") || name.contains("(")

    /** What two listings of the same film or series have in common: the kind,
     *  the name without what follows a bracket and without marks or case, and
     *  the year (from `year`, else a "(2024)" in the name). */
    fun titleKey(kind: String, name: String, year: String?): String {
        val parts = name.split('(', '[')
        val first = parts.first().trim()
        val head = first.ifEmpty { name }
        val plain = head.filter { it.isLetterOrDigit() }.lowercase()
        fun four(text: String): String? {
            val digits = text.take(4)
            return if (digits.length == 4 && digits.all { it in '0'..'9' }) digits else null
        }
        val fromYear = year?.let { four(it) }
        val fromName = parts.drop(1).firstNotNullOfOrNull { part ->
            four(part)?.takeIf { it.startsWith("19") || it.startsWith("20") }
        }
        return "$kind|$plain|${fromYear ?: fromName ?: ""}"
    }

    /** Drops repeats of the same film or series, keeping each one's first
     *  place. Of two listings, the one named like a title wins over the one
     *  named like a file. */
    fun withoutRepeats(titles: List<MetaPreview>): List<MetaPreview> {
        val kept = mutableListOf<MetaPreview>()
        val places = mutableMapOf<String, Int>()
        for (title in titles) {
            val key = titleKey(title.kind, title.name, title.releaseInfo)
            val at = places[key]
            if (at != null) {
                if (isReleaseName(kept[at].name) && !isReleaseName(title.name)) kept[at] = title
            } else {
                places[key] = kept.size
                kept.add(title)
            }
        }
        return kept
    }

    /** Reads a catalogue answer. Entries missing an id, type or name are left out. */
    fun parseCatalog(json: String): List<MetaPreview> {
        val page = top(json)
        val metas = page["metas"] ?: return emptyList()
        val list = metas as? JsonArray ?: throw TitlesError.BadResponse("metas is not a list")
        return list.mapNotNull { item -> JObj.of(item)?.let { runCatching { MetaPreview.from(it) }.getOrNull() } }
    }

    /** Reads a `meta` answer. Episodes with no id are left out. */
    fun parseMeta(json: String): MetaDetail {
        val answer = top(json)
        val meta = JObj.of(answer["meta"])?.takeIf { it.raw.isNotEmpty() } ?: throw TitlesError.UnknownTitle()
        return try {
            MetaDetail.from(meta)
        } catch (wrong: JObj.Wrong) {
            throw TitlesError.BadResponse("${wrong.field} has the wrong type")
        }
    }

    /** The top-level object of an answer. */
    fun top(json: String): JObj = try {
        JObj.parse(json)
    } catch (wrong: JObj.Wrong) {
        throw TitlesError.BadResponse("${wrong.field} is not an object")
    } catch (_: Exception) {
        throw TitlesError.BadResponse("not JSON")
    }
}
