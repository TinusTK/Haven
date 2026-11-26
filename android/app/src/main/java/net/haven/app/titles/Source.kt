package net.haven.app.titles

import net.haven.app.core.Http
import net.haven.app.core.HttpError
import net.haven.app.core.Log
import net.haven.app.core.Store

/** One kind of answer a source gives, for which types and ids. */
data class Serves(
    /** `catalog`, `meta`, `stream`… */
    val resource: String,
    val types: List<String>,
    /** If set, only ids that start with one of these. */
    val idPrefixes: List<String>?,
)

/** A source of catalogues and details: Haven's built-in catalogue, or an
 *  add-on (installed, or brought in by the Stremio sign-in). */
data class Source(
    /** Its description's address; everything else it serves sits next to it. */
    val url: String,
    val name: String,
    val catalogs: List<Catalog>,
    val serves: List<Serves>,
) {
    /** Whether this source can be asked for `resource` about the title `id` of type `kind`. */
    fun provides(resource: String, kind: String, id: String): Boolean = serves.any { s ->
        s.resource == resource && kind in s.types && (s.idPrefixes?.any { id.startsWith(it) } ?: true)
    }
}

/** Cinemeta: the free catalogue of films and series built into Haven. It
 *  needs no key, so Home and Explore have something in them from the start. */
object Cinemeta {
    const val URL = "https://v3-cinemeta.strem.io/manifest.json"
    const val NAME = "Cinemeta"
    /** Where its last description is kept (kv), as raw JSON text. */
    const val KEPT_KEY = "cinemeta_description"

    /** The built-in catalogue, with the catalogues it lists (from its own
     *  description, or the plain list below when that is not known yet). */
    fun source(catalogs: List<Catalog>? = null): Source {
        val types = listOf("movie", "series")
        return Source(URL, NAME, catalogs ?: fallback, listOf("catalog", "meta").map { Serves(it, types, listOf("tt")) })
    }

    /** Popular and Featured films and series, searchable, with pages. */
    val fallback: List<Catalog>
        get() = listOf("top" to "Popular", "imdbRating" to "Featured").flatMap { (id, name) ->
            listOf("movie", "series").map { kind ->
                Catalog(kind, id, name, extraSupported = if (id == "top") listOf("skip", "search") else listOf("skip"))
            }
        }

    /** The built-in catalogue as it was last described, if it was. */
    fun kept(store: Store): Source {
        val json = runCatching { store.get<String>(KEPT_KEY) }.getOrNull() ?: return source()
        val catalogs = runCatching { Catalog.parseList(json) }.getOrNull()?.takeIf { it.isNotEmpty() } ?: return source()
        return source(catalogs)
    }

    /** Asks Cinemeta which catalogues it has now. Returns the new source (and
     *  keeps its description) only when it changed from `current`. */
    suspend fun refresh(store: Store, current: Source, catalogues: Catalogues = Catalogues()): Source? {
        val json = runCatching { catalogues.descriptionJson(URL) }.getOrNull() ?: return null
        val catalogs = runCatching { Catalog.parseList(json) }.getOrNull()?.takeIf { it.isNotEmpty() } ?: return null
        val fresh = source(catalogs)
        if (fresh == current) return null
        runCatching { store.put(KEPT_KEY, json) }.onFailure { Log.note("could not keep the catalogue list: ${it.message}") }
        return fresh
    }
}

/** Reads catalogues and details from a catalogue service over the web. */
class Catalogues(private val http: Http = Http.shared) {
    companion object {
        const val DESCRIPTION_LIMIT = 1024 * 1024
        const val ANSWER_LIMIT = 8 * 1024 * 1024
    }

    suspend fun text(url: String, limit: Int): String {
        val body = try {
            http.getBytes(url, limit)
        } catch (e: HttpError) {
            throw TitlesError.Http(e)
        }
        return runCatching { Charsets.UTF_8.newDecoder().decode(java.nio.ByteBuffer.wrap(body)).toString() }
            .getOrElse { throw TitlesError.NotText() }
    }

    /** The service's own description as raw JSON. */
    suspend fun descriptionJson(url: String): String = text(url, DESCRIPTION_LIMIT)

    /** The first page of a catalogue, or of its search results. */
    suspend fun catalog(sourceUrl: String, catalog: Catalog, search: String? = null): List<MetaPreview> =
        Titles.parseCatalog(text(ResourceUrl.catalog(sourceUrl, catalog, search), ANSWER_LIMIT))

    /** A page of a catalogue: narrowed to `genre` if given, starting `skip` titles in. */
    suspend fun catalogPage(sourceUrl: String, catalog: Catalog, genre: String?, skip: Int): List<MetaPreview> {
        val extras = buildList {
            if (genre != null) add("genre" to genre)
            if (skip != 0) add("skip" to skip.toString())
        }
        return Titles.parseCatalog(text(ResourceUrl.make(sourceUrl, "catalog", catalog.kind, catalog.id, extras), ANSWER_LIMIT))
    }

    /** Everything the source knows about one title. */
    suspend fun meta(sourceUrl: String, kind: String, id: String): MetaDetail =
        Titles.parseMeta(text(ResourceUrl.make(sourceUrl, "meta", kind, id), ANSWER_LIMIT))
}
