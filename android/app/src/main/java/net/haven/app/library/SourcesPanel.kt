package net.haven.app.library

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import net.haven.app.addons.AddonClient
import net.haven.app.core.Http
import net.haven.app.core.Log
import net.haven.app.livetv.LiveTV
import net.haven.app.livetv.XtreamOnDemand
import net.haven.app.pictures.Generation
import net.haven.app.servers.ServerAccounts
import net.haven.app.servers.Servers
import net.haven.app.servers.Wanted
import net.haven.app.titles.Catalog
import net.haven.app.titles.Catalogues
import net.haven.app.titles.MediaStream
import net.haven.app.titles.MetaDetail
import java.util.concurrent.atomic.AtomicLong

/**
 * Where a film or episode can be watched: every version of it on every media
 * server that has it, every stream the add-ons offer for it, and the Xtream
 * Codes accounts' films and series. The Rust build's `library/sources.rs`.
 */
class SourcesPanel(private val library: Library, private val scope: CoroutineScope) {
    /** The film or episode listed. For an episode, `detail` says which one
     *  ("S1 E2 · Name"); empty for a film. */
    data class Listed(val videoId: String, val detail: String = "")

    /** What is asked about: the open title and the film or episode. */
    data class Asking(val kind: String, val titleId: String, val videoId: String, val meta: MetaDetail?, val name: String)

    var heading by mutableStateOf(TitleRules.HEADING)
        private set
    var loading by mutableStateOf(false)
        private set
    var note by mutableStateOf("")
        private set
    var rows by mutableStateOf(listOf<StreamRow>())
        private set
    var groups by mutableStateOf(listOf<Pair<String, Int>>())
        private set
    /** The provider chosen above the list; empty for all. */
    var group by mutableStateOf("")
    var listed by mutableStateOf<Listed?>(null)
        private set

    /** The next episode plays from the same place as the last (its source and
     *  the row's name) as soon as that row shows. */
    var autoplay: Pair<String, String>? = null
    /** Plays a row (the title page's Play). */
    var playRow: (StreamRow) -> Unit = {}
    /** The next episode's own place did not turn up: the player gives way to
     *  this list. */
    var giveUp: () -> Unit = {}

    private var serverRows = 0
    private val limits = HashMap<String, Int>()
    private var pending = 0
    private var nextId = 0
    private val generation = Generation()
    /** Tells a long list still arriving from a media server to stop. */
    private val current = AtomicLong(0)

    /** Empties the list (another title opened, or the page closed). */
    fun clear() {
        generation.next()
        current.set(generation.current)
        rows = emptyList()
        serverRows = 0
        limits.clear()
        pending = 0
        loading = false
        note = ""
        heading = TitleRules.HEADING
        listed = null
        showGroups()
    }

    /** Says something over an empty list ("Choose an episode…"). */
    fun say(words: String, heading: String = TitleRules.HEADING) {
        clear()
        note = words
        this.heading = heading
    }

    /** Lists every place the film or episode can be watched: asks each media
     *  server whether it has it, every add-on that offers streams for it, and
     *  the Xtream Codes accounts. */
    fun load(asking: Asking, heading: String) {
        clear()
        val token = generation.current
        listed = Listed(asking.videoId)
        this.heading = heading
        val wanted = TitleRules.wanted(asking.kind, asking.videoId, asking.titleId, asking.meta, asking.name)
        // A series can only be matched down to an episode.
        val canMatch = wanted.name.isNotBlank() && (!wanted.series || wanted.episode != null)
        val servers = library.servers.enabled
        val origin = ServerAccounts.serverItem(asking.videoId, servers)
        // A title a server gathered under a TMDB or other number has no
        // sources under that name: it is looked up again, by its IMDb id.
        val renamed = Servers.splitId(asking.titleId)?.let { Servers.wantsImdb(it.second) } ?: false
        // Where an IMDb id can be looked up by name when none is known.
        val lookups = library.installed.flatMap { source ->
            source.catalogs.filter { it.kind == asking.kind && it.searchable }.map { source.url to it }
        }
        var asked = 0
        val client = library.serverClient
        val catalogues = library.catalogues

        for (server in servers) {
            // The server the title was opened from holds it for certain; the
            // others are asked whether they have the same title.
            val own = origin?.takeIf { it.first.id == server.id && !renamed }?.second
            // Every server is asked, even one marked offline earlier.
            if (own == null && !canMatch) continue
            asked += 1
            val source = "${server.name} (${server.kind.label})"
            scope.launch {
                val found = runCatching {
                    var item = own
                    if (item == null) {
                        val ask = if (wanted.imdb == null) wanted.copy(imdb = imdbByName(catalogues, lookups, wanted)) else wanted
                        item = client.find(server, ask)
                    }
                    if (item != null) {
                        val id = Servers.titleId(server.id, item)
                        // A long list shows as it arrives, a few at a time.
                        client.streamsLive(server, item) { some ->
                            scope.launch { arrived(token, source, id, some) }
                            current.get() == token
                        }
                    }
                }
                found.fold(
                    onSuccess = {
                        // It answered: online (its shelves come back if it was not).
                        library.serverAnswered(server.id)
                        loaded(token, source, null, Result.success(emptyList()))
                    },
                    onFailure = { loaded(token, source, null, Result.failure(it)) },
                )
            }
        }

        // The add-ons: GitHub's installed ones, and the Stremio account's in both builds.
        TitleRules.addonId(asking.videoId, wanted)?.let { addonId ->
            val addons = AddonClient(catalogues)
            for (source in library.installed) {
                if (!source.provides("stream", asking.kind, addonId)) continue
                asked += 1
                scope.launch {
                    val result = runCatching { addons.streams(source.url, asking.kind, addonId) }
                    loaded(token, source.name, null, result)
                }
            }
        }

        // The films and series of the user's Xtream Codes accounts.
        if (XtreamOnDemand.worthAsking(wanted)) {
            for (account in LiveTV.accounts(library.store)) {
                asked += 1
                scope.launch {
                    val result = library.onDemand.streams(account, wanted)
                    loaded(token, XtreamOnDemand.sourceLabel(account), null, result)
                }
            }
        }

        pending = asked
        if (asked == 0) {
            giveUpAutoplay()
            note = TitleRules.nothingCanPlay
        } else {
            loading = true
        }
    }

    /** Some places to watch came in (a media server may send more). */
    fun arrived(token: Long, source: String, serverItem: String?, streams: List<MediaStream>) {
        if (!generation.isCurrent(token)) return
        val list = rows.toMutableList()
        for (stream in streams) {
            val row = TitleRules.streamRow(nextId, source, stream)?.copy(serverItem = serverItem) ?: continue
            nextId += 1
            list.add(TitleRules.place(row, list, serverRows, serverItem != null), row)
            if (serverItem != null) serverRows += 1
        }
        rows = TitleRules.marked(list, limits)
        showGroups()
        autoplayFrom(source, finished = false)
    }

    /** Plays the next episode from `source` if that is where the last one
     *  played from: the row of the same name or, once the source has
     *  `finished` answering, its first playable row. */
    private fun autoplayFrom(source: String, finished: Boolean) {
        val (wantedSource, wantedName) = autoplay ?: return
        if (wantedSource != source) return
        val candidates = rows.filter { it.playable && it.source == wantedSource }
        val pick = candidates.firstOrNull { it.name == wantedName } ?: (if (finished) candidates.firstOrNull() else null) ?: return
        autoplay = null
        playRow(pick)
    }

    private fun giveUpAutoplay() {
        if (autoplay == null) return
        autoplay = null
        giveUp()
    }

    /** One media server, add-on or account has finished answering. */
    fun loaded(token: Long, source: String, serverItem: String?, result: Result<List<MediaStream>>) {
        if (!generation.isCurrent(token)) return
        result.fold(
            onSuccess = { arrived(token, source, serverItem, it) },
            onFailure = { Log.note("$source gave no places to watch: ${Http.words(it.cause ?: it)}") },
        )
        autoplayFrom(source, finished = true)
        pending = maxOf(pending - 1, 0)
        if (pending == 0) {
            giveUpAutoplay()
            loading = false
            note = if (rows.isEmpty()) TitleRules.NONE_HAS_IT else ""
        }
    }

    /** Shows ten more of a provider's places to watch. */
    fun showMore(group: String) {
        limits[group] = (limits[group] ?: TitleRules.STREAMS_SHOWN) + TitleRules.STREAMS_SHOWN
        rows = TitleRules.marked(rows, limits)
    }

    /** The rows the list shows now: ten per provider, or the chosen one's. */
    val shownRows: List<StreamRow> get() = rows.filter { it.shown && (group.isEmpty() || it.group == group) }

    /** Media servers, add-ons or accounts still to answer. */
    val stillAsking get() = pending > 0

    fun setDetail(detail: String) {
        listed = listed?.copy(detail = detail)
    }

    private fun showGroups() {
        groups = TitleRules.groups(rows)
        // A chosen provider that is gone falls back to all.
        if (groups.none { it.first == group }) group = ""
        if (rows.isEmpty()) limits.clear()
    }

    companion object {
        /** Looks a title's IMDb id up by its name and year in the catalogues
         *  that can be searched (the built-in one, for a start). */
        suspend fun imdbByName(catalogues: Catalogues, lookups: List<Pair<String, Catalog>>, wanted: Wanted): String? {
            val name = wanted.name.trim()
            if (name.isEmpty()) return null
            for ((url, catalog) in lookups) {
                val found = runCatching { catalogues.catalog(url, catalog, name) }.getOrNull() ?: continue
                TitleRules.imdbMatch(found, name, wanted.year)?.let { return it }
            }
            return null
        }
    }
}
