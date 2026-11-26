package net.haven.app.library

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.serialization.builtins.ListSerializer
import net.haven.app.core.Http
import net.haven.app.metadata.Genre
import net.haven.app.metadata.Tmdb
import net.haven.app.pictures.Generation
import net.haven.app.pictures.Kept
import net.haven.app.servers.MediaLibrary
import net.haven.app.servers.Server
import net.haven.app.servers.ServerClient
import net.haven.app.titles.Catalog
import net.haven.app.titles.Catalogues
import net.haven.app.titles.MetaPreview
import net.haven.app.titles.Titles

/** Somewhere Explore can browse: the built-in TMDB catalogue, a library of a
 *  media server, or a catalogue of Cinemeta or an add-on. */
sealed class ExploreSource {
    data object TmdbAll : ExploreSource()
    data class OnServer(val server: Server, val library: MediaLibrary) : ExploreSource()
    data class Catalogue(val url: String, val source: String, val catalog: Catalog) : ExploreSource()

    /** Where it comes from: the heading it is listed under. */
    val origin: String get() = when (this) {
        TmdbAll -> "TMDB"
        is OnServer -> server.name
        is Catalogue -> source
    }

    /** Its name under that heading. */
    val name: String get() = when (this) {
        TmdbAll -> "Films and series"
        is OnServer -> library.name
        is Catalogue -> catalog.title
    }

    /** Tells sources apart, whatever their place in the list. */
    val label: String get() = "$name  ·  $origin"
}

/** A line in the list of sources: a heading, or a source by its place. */
data class ExploreRow(
    /** The source's place in the list; -1 for a heading. */
    val index: Int,
    val label: String,
) {
    val header get() = index < 0
    val id get() = if (header) "h|$label" else "$index"
}

/**
 * The Explore page: one grid of posters to browse everything there is, a
 * source at a time, narrowed to films or series (TMDB) and a genre. The Rust
 * build's `library/explore.rs`.
 */
class Explore(private val library: Library, private val scope: CoroutineScope) {
    companion object {
        /** Titles asked for at a time from a media server. */
        const val SERVER_PAGE = 60
        /** TMDB hands out 20 titles a page; two pages fill a screen. */
        const val TMDB_PAGES_AT_ONCE = 2

        /** The genres as the chooser lists them: "All genres" first, which
         *  stands for no genre. Empty when there are none to choose from. */
        fun choices(names: List<String>) = if (names.isEmpty()) emptyList() else listOf("All genres") + names

        /** The grid with `titles` added: pages can overlap, and the same film
         *  can be listed again on a later page under another id. */
        fun adding(titles: List<MetaPreview>, items: List<MetaPreview>): Pair<List<MetaPreview>, Int> {
            val list = items.toMutableList()
            val ids = items.map { "${it.kind}|${it.id}" }.toMutableSet()
            val keys = items.map { Titles.titleKey(it.kind, it.name, it.releaseInfo) }.toMutableSet()
            var added = 0
            for (title in Titles.withoutRepeats(titles)) {
                val key = Titles.titleKey(title.kind, title.name, title.releaseInfo)
                if (!ids.add("${title.kind}|${title.id}")) continue
                if (!keys.add(key)) continue
                list += title
                added += 1
            }
            return list to added
        }

        /** A page of titles, where the page after it starts, and whether
         *  there may be one. */
        data class Page(val titles: List<MetaPreview>, val next: Int, val more: Boolean)

        suspend fun fetch(
            source: ExploreSource,
            start: Int,
            genre: Int,
            series: Boolean,
            tmdbGenre: Long?,
            serverGenre: String?,
            tmdb: Tmdb?,
            client: ServerClient,
            catalogues: Catalogues,
        ): Result<Page> = runCatching {
            when (source) {
                ExploreSource.TmdbAll -> {
                    tmdb ?: error("there is no TMDB key")
                    val titles = mutableListOf<MetaPreview>()
                    for (number in start until start + TMDB_PAGES_AT_ONCE) {
                        try {
                            titles += tmdb.discover(series, tmdbGenre, number)
                        } catch (e: Exception) {
                            if (titles.isEmpty()) throw e
                            break
                        }
                    }
                    Page(titles, start + TMDB_PAGES_AT_ONCE, titles.size >= 20 * TMDB_PAGES_AT_ONCE)
                }
                is ExploreSource.OnServer -> {
                    val titles = client.page(source.server, source.library.id, serverGenre, start, SERVER_PAGE)
                    Page(titles, start + SERVER_PAGE, titles.size >= SERVER_PAGE)
                }
                is ExploreSource.Catalogue -> {
                    val named = source.catalog.genres.getOrNull(genre)
                    val titles = catalogues.catalogPage(source.url, source.catalog, named, start)
                    Page(titles, start + titles.size, source.catalog.pages && titles.isNotEmpty())
                }
            }
        }
    }

    // What the page shows.
    var sources by mutableStateOf(listOf<ExploreSource>())
        private set
    /** Words the listed sources must hold. */
    var words by mutableStateOf("")
        private set
    var rows by mutableStateOf(listOf<ExploreRow>())
        private set
    /** The chosen source's place in `sources`; -1 when there is none. */
    var source by mutableIntStateOf(-1)
        private set
    /** "Films", "Series" for TMDB; empty for the others. */
    var kinds by mutableStateOf(listOf<String>())
        private set
    /** 0 films, 1 series (TMDB only). */
    var kind by mutableIntStateOf(0)
        private set
    /** "All genres" then the source's genres; empty when there are none. */
    var genres by mutableStateOf(listOf<String>())
        private set
    /** The chosen genre's place (without "All genres"); -1 for all. */
    var genre by mutableIntStateOf(-1)
        private set
    var items by mutableStateOf(listOf<MetaPreview>())
        private set
    var loading by mutableStateOf(false)
        private set
    /** There may be another page. */
    var more by mutableStateOf(false)
        private set
    var note by mutableStateOf("")
        private set
    var refreshing by mutableStateOf(false)
        private set

    // The search and the source list on top of the grid (kept while other
    // pages show, as in the Rust build).
    var query by mutableStateOf("")
    var searching by mutableStateOf(false)
    var picking by mutableStateOf(false)

    // Behind the page.
    /** The chosen source's label, to find it again when the list changes. */
    private var chosen = ""
    private var tmdbGenres = listOf<Genre>()
    private var serverGenres = listOf<String>()
    private val generation = Generation()
    /** Where the next page starts: a page number for TMDB, else how many
     *  titles to skip. */
    private var next = 0
    /** The first page of this source has not come yet. */
    private var first = false
    /** The grid shows the first page kept from last time (or the titles that
     *  showed before a refresh). */
    private var showingKept = false
    private var keptName = ""
    /** The page has been opened: nothing is fetched before that. */
    private var wanted = false
    private val previews = ListSerializer(MetaPreview.serializer())

    // Sources

    /** Lists the sources again (an add-on, a server or the TMDB key came or
     *  went). The chosen source stays chosen if it is still there. */
    fun rebuild() {
        val list = mutableListOf<ExploreSource>()
        if (library.tmdb != null) list += ExploreSource.TmdbAll
        list += library.serverLibraries.map { (server, lib) -> ExploreSource.OnServer(server, lib) }
        for (found in library.installed) {
            list += found.catalogs.filter { it.browsable }.map { ExploreSource.Catalogue(found.url, found.name, it) }
        }
        val still = list.indexOfFirst { it.label == chosen }
        sources = list
        listRows()
        // Still there: only its place in the list may have moved.
        if (still >= 0) source = still else pick(0, 0, -1)
    }

    fun findSource(text: String) {
        if (text == words) return
        words = text
        listRows()
    }

    /** The list of sources: a heading for each place they come from, then its
     *  sources that hold the words looked for. */
    private fun listRows() {
        val wanted = words.lowercase().split(Regex("\\s+")).filter { it.isNotEmpty() }
        val list = mutableListOf<ExploreRow>()
        var heading: String? = null
        sources.forEachIndexed { index, found ->
            val label = found.label.lowercase()
            if (!wanted.all { label.contains(it) }) return@forEachIndexed
            if (found.origin != heading) {
                heading = found.origin
                list += ExploreRow(-1, found.origin)
            }
            list += ExploreRow(index, found.name)
        }
        rows = list
    }

    // Choosing

    /** Shows a source from its start, narrowed to a kind and a genre. */
    fun pick(index: Int, kind: Int, genre: Int) {
        generation.next()
        items = emptyList()
        loading = false
        val at = maxOf(index, 0)
        if (at !in sources.indices) {
            chosen = ""
            source = -1
            kinds = emptyList()
            genres = emptyList()
            note = ""
            more = false
            return
        }
        val picked = sources[at]
        val sameKind = chosen == picked.label && this.kind == maxOf(kind, 0)
        chosen = picked.label
        source = at
        this.kind = maxOf(kind, 0)
        this.genre = genre
        note = ""
        when (picked) {
            ExploreSource.TmdbAll -> {
                next = 1
                kinds = listOf("Films", "Series")
                if (!sameKind) {
                    // Another kind has other genres: fetch them.
                    tmdbGenres = emptyList()
                    genres = emptyList()
                    fetchTmdbGenres(this.kind == 1)
                }
            }
            is ExploreSource.OnServer -> {
                next = 0
                kinds = emptyList()
                if (!sameKind) {
                    serverGenres = emptyList()
                    genres = emptyList()
                    fetchServerGenres(picked.server, picked.library, picked.label)
                }
            }
            is ExploreSource.Catalogue -> {
                next = 0
                kinds = emptyList()
                genres = choices(picked.catalog.genres)
                // A catalogue that only answers with a genre starts on its first.
                if (genre < 0 && picked.catalog.needsInput) this.genre = 0
            }
        }
        // The first page as it was last time shows at once; the fresh one
        // replaces it when it comes.
        keptName = Kept.explore(picked.label, this.kind.toString(), this.genre.toString())
        first = true
        showingKept = false
        library.kept.read(keptName, previews)?.takeIf { it.isNotEmpty() }?.let {
            add(it)
            showingKept = true
        }
        if (wanted) load()
    }

    fun chooseSource(index: Int) {
        pick(index, 0, -1)
        picking = false
    }

    fun chooseKind(kind: Int) = pick(source, kind, -1)

    /** `choice` is the place in `genres`: 0 is "All genres". */
    fun chooseGenre(choice: Int) = pick(source, kind, choice - 1)

    /** The page came into view: the grid starts loading the first time. */
    fun appeared() {
        if (wanted) return
        wanted = true
        if (first && !loading) load()
    }

    private fun fetchTmdbGenres(series: Boolean) {
        val tmdb = library.tmdb ?: return
        scope.launch {
            val found = runCatching { tmdb.genres(series) }.getOrNull() ?: return@launch
            // Still the same source and kind.
            if (kind != (if (series) 1 else 0) || chosen != ExploreSource.TmdbAll.label) return@launch
            tmdbGenres = found
            genres = choices(found.map { it.name })
        }
    }

    private fun fetchServerGenres(server: Server, lib: MediaLibrary, label: String) {
        val client = library.serverClient
        scope.launch {
            val found = runCatching { client.genres(server, lib.id) }.getOrDefault(emptyList())
            if (chosen != label || found.isEmpty()) return@launch
            serverGenres = found
            genres = choices(found)
        }
    }

    // Pages

    /** Loads the source showing afresh (Explore pulled down). The titles on
     *  screen stay until the fresh first page replaces them; returns when it
     *  has come (or after 20 seconds). */
    suspend fun refresh() {
        if (source !in sources.indices || sources[source].label != chosen) return
        refreshing = true
        val onScreen = items
        val wasKept = showingKept
        pick(source, kind, genre)
        if (onScreen.isNotEmpty() && !wasKept && !showingKept) {
            items = onScreen
            showingKept = true
        }
        val started = System.currentTimeMillis()
        while (first && System.currentTimeMillis() - started < 20_000) delay(150)
        refreshing = false
    }

    /** Fetches the next page of the chosen source and adds it to the grid. */
    fun load() {
        if (loading || source !in sources.indices || sources[source].label != chosen) return
        val picked = sources[source]
        val token = generation.current
        val start = next
        val genre = genre
        val series = kind == 1
        loading = true
        val tmdbGenre = tmdbGenres.getOrNull(genre)?.id
        val serverGenre = serverGenres.getOrNull(genre)
        val tmdb = library.tmdb
        scope.launch {
            val page = fetch(picked, start, genre, series, tmdbGenre, serverGenre, tmdb, library.serverClient, library.catalogues)
            loaded(token, page)
        }
    }

    private fun loaded(token: Long, result: Result<Page>) {
        if (!generation.isCurrent(token)) return
        loading = false
        val wasFirst = first
        first = false
        val page = result.getOrElse { problem ->
            if (wasFirst && showingKept) {
                // The kept page stays when the fresh one cannot be had.
                showingKept = false
                more = false
                return
            }
            more = false
            note = "Could not load this: ${problem.cause?.let(Http::words) ?: Http.words(problem)}."
            return
        }
        next = page.next
        if (wasFirst) {
            library.kept.write(keptName, page.titles, previews)
            // The fresh first page takes the kept one's place.
            if (showingKept) {
                showingKept = false
                items = emptyList()
            }
        }
        val added = add(page.titles)
        // A page of nothing new means the end, whatever the source says.
        more = page.more && added > 0
        note = if (items.isEmpty()) "Nothing here." else ""
    }

    /** Adds titles to the grid, each once; returns how many were added. */
    private fun add(titles: List<MetaPreview>): Int {
        val (list, added) = adding(titles, items)
        items = list
        return added
    }
}
