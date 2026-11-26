package net.haven.app.titles

import kotlinx.serialization.json.JsonArray
import net.haven.app.core.Http

/** An option a catalogue accepts, such as `search`, `genre` or `skip`. */
data class Extra(
    val name: String,
    val isRequired: Boolean = false,
    /** The values it takes, such as the genres to choose from. */
    val options: List<String> = emptyList(),
) {
    companion object {
        fun from(o: JObj) = Extra(o.string("name"), o.bool("isRequired"), o.texts("options"))
    }
}

/** One browsable list a catalogue service offers, such as "Popular" films. */
data class Catalog(
    /** Content type: `movie`, `series`, `channel`, `tv`… */
    val kind: String,
    val id: String,
    val name: String? = null,
    val extra: List<Extra> = emptyList(),
    /** Older way of naming the extras a catalogue accepts. */
    val extraSupported: List<String> = emptyList(),
    /** Older way of naming required extras. */
    val extraRequired: List<String> = emptyList(),
) {
    private val required: List<String> get() = extra.filter { it.isRequired }.map { it.name } + extraRequired

    /** True when the catalogue only answers with extra input (a search term,
     *  a genre…), so it cannot be shown as a plain shelf. */
    val needsInput: Boolean get() = required.isNotEmpty()

    /** True when the catalogue can be searched with nothing but a search term. */
    val searchable: Boolean
        get() = (extra.any { it.name == "search" } || "search" in extraSupported) && required.all { it == "search" }

    /** The genres this catalogue can be narrowed to; empty if it has none. */
    val genres: List<String> get() = extra.firstOrNull { it.name == "genre" }?.options ?: emptyList()

    /** True when the catalogue hands out further pages (`skip`). */
    val pages: Boolean get() = extra.any { it.name == "skip" } || "skip" in extraSupported

    /** True when the catalogue can be browsed with at most a genre chosen. */
    val browsable: Boolean get() = required.all { it == "genre" && genres.isNotEmpty() }

    /** What this catalogue's content is called: "films", "series"… */
    val kindLabel: String
        get() = when (kind) {
            "movie" -> "films"
            "series" -> "series"
            "channel" -> "channels"
            "tv" -> "TV"
            else -> kind
        }

    /** A shelf title such as "Popular films". */
    val title: String
        get() = name?.trim()?.takeIf { it.isNotEmpty() }?.let { "$it $kindLabel" } ?: capitalised(kindLabel)

    /** A title for this catalogue's search results, such as "Films". */
    val resultsTitle: String get() = capitalised(kindLabel)

    companion object {
        private fun capitalised(text: String): String {
            val first = text.firstOrNull() ?: return text
            return if (first.code < 128) first.uppercase() + text.drop(1) else text
        }

        fun from(o: JObj): Catalog {
            val extra = when (val list = o["extra"]) {
                null -> emptyList()
                is JsonArray -> list.map { Extra.from(JObj.of(it) ?: throw JObj.Wrong("extra")) }
                else -> throw JObj.Wrong("extra")
            }
            return Catalog(
                kind = o.string("type"), id = o.string("id"), name = o.optionalString("name"), extra = extra,
                extraSupported = o.strings("extraSupported"), extraRequired = o.strings("extraRequired"),
            )
        }

        /** Reads the catalogues a service lists in its description
         *  (`{"catalogs": [...]}`); other fields are ignored. */
        fun parseList(json: String): List<Catalog> {
            val described = Titles.top(json)
            val list = described["catalogs"] ?: return emptyList()
            return try {
                val items = list as? JsonArray ?: throw JObj.Wrong("catalogs")
                items.map { from(JObj.of(it) ?: throw JObj.Wrong("catalogs")) }
            } catch (wrong: JObj.Wrong) {
                throw TitlesError.BadResponse("${wrong.field} has the wrong type")
            }
        }
    }
}

/** Addresses of what a service serves next to its description (`…/manifest.json`). */
object ResourceUrl {
    /** Percent-encodes one part of a path the way catalogue services expect:
     *  everything except letters, digits and `-_.~`. */
    fun encoded(part: String): String = buildString {
        for (byte in part.toByteArray(Charsets.UTF_8)) {
            val b = byte.toInt() and 0xff
            val c = b.toChar()
            if (b < 128 && (c.isLetterOrDigit() || c in "-_.~")) append(c) else append("%%%02X".format(b))
        }
    }

    /** `<resource>/<kind>/<id>.json`, or with extra options
     *  `<resource>/<kind>/<id>/<name>=<value>&<name>=<value>.json`. */
    fun make(descriptionUrl: String, resource: String, kind: String, id: String, extras: List<Pair<String, String>> = emptyList()): String {
        val invalid = TitlesError.InvalidAddress(descriptionUrl.trim())
        val url = runCatching { Http.webUrl(descriptionUrl) }.getOrNull() ?: throw invalid
        if (!url.encodedPath.endsWith("manifest.json")) throw invalid
        val path = buildString {
            append(url.encodedPath.removeSuffix("manifest.json"))
            append("${encoded(resource)}/${encoded(kind)}/${encoded(id)}")
            extras.forEachIndexed { at, (name, value) ->
                append(if (at == 0) "/" else "&")
                append("${encoded(name)}=${encoded(value)}")
            }
            append(".json")
        }
        return url.newBuilder().encodedPath(path).build().toString()
    }

    /** The address of one of the service's catalogues, optionally searched. */
    fun catalog(descriptionUrl: String, catalog: Catalog, search: String? = null): String =
        make(descriptionUrl, "catalog", catalog.kind, catalog.id, search?.let { listOf("search" to it) } ?: emptyList())
}
