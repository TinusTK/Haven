package net.haven.app.library

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.builtins.ListSerializer
import net.haven.app.BuildConfig
import net.haven.app.addons.InstalledAddons
import net.haven.app.core.DeviceId
import net.haven.app.core.Http
import net.haven.app.core.Log
import net.haven.app.core.Saved
import net.haven.app.core.Secrets
import net.haven.app.core.Store
import net.haven.app.core.WatchProgress
import net.haven.app.metadata.Tmdb
import net.haven.app.pictures.Kept
import net.haven.app.pictures.Pause
import net.haven.app.servers.MediaLibrary
import net.haven.app.servers.Server
import net.haven.app.servers.ServerAccounts
import net.haven.app.servers.ServerClient
import net.haven.app.titles.Catalogues
import net.haven.app.titles.Cinemeta
import net.haven.app.titles.MetaDetail
import net.haven.app.titles.MetaPreview
import net.haven.app.titles.Source
import net.haven.app.titles.Titles
import net.haven.app.tracking.ImdbLink
import java.time.Instant
import java.time.ZoneId

/** What opening a title needs (from a card, the banner, Upcoming…). The
 *  title's page comes in K12. */
data class Opening(
    val id: String,
    val kind: String,
    val title: String,
    /** The episode to show (from Continue Watching or Upcoming). */
    val video: String? = null,
    val poster: String? = null,
)

/**
 * Haven's titles: where they come from (Cinemeta, add-ons, media servers,
 * TMDB), Home's shelves, the banner, Continue Watching, the Watchlist and
 * when its titles come out (the Rust build's `Library`). Its state is read
 * by the screens and changed on the main thread; the network is asked in
 * the background.
 */
class Library(
    val store: Store,
    keys: Secrets,
    val http: Http = Http.shared,
    val kept: Kept,
    private val scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate),
) {
    var keys by mutableStateOf(keys)
        private set
    val catalogues = Catalogues(http)
    val serverClient = ServerClient(DeviceId.get(store), http)
    /** Home's work waits while Home is not showing. */
    val homePause = Pause()
    private val serverGate = Semaphore(ShelfLength.SERVERS_AT_ONCE)
    private val calendarGate = Semaphore(2)
    /** Seconds since 1970, and the time zone (stand-ins in tests). */
    var now: () -> Long = { System.currentTimeMillis() / 1000 }
    var zone: ZoneId = ZoneId.systemDefault()

    // Sources

    var builtin by mutableStateOf(Cinemeta.kept(store))
        private set
    var addons by mutableStateOf(InstalledAddons())
        private set
    /** Cinemeta first, then the switched-on add-ons. */
    val installed: List<Source> get() = listOf(builtin) + addons.sources

    var servers by mutableStateOf(ServerAccounts.Loaded())
        private set
    /** Each switched-on server's libraries, as last listed. */
    var serverLibraries by mutableStateOf(listOf<Pair<Server, MediaLibrary>>())
        private set
    var serverStates by mutableStateOf(mapOf<Long, ServerAccounts.State>())
        private set

    /** TMDB, when a key is set. */
    val tmdb: Tmdb? get() = keys.tmdbApiKey?.let { Tmdb(it, http) }

    // Boards

    val homeBoard = ShelfBoard(Board.Home)
    val serverBoard = ShelfBoard(Board.Servers)
    val catalogBoard = ShelfBoard(Board.Catalog)
    val searchBoard = ShelfBoard(Board.Search)
    /** Why search shows nothing (nothing added that can be searched). */
    var searchNote by mutableStateOf("")
        private set
    /** Home's requests still out (pull to refresh waits for them). */
    var fetchesLeft by mutableIntStateOf(0)
        private set
    var refreshing by mutableStateOf(false)
        private set

    // Home's top

    var hero by mutableStateOf(listOf<HomeTop.Slide>())
        private set
    private var heroStarted = false
    private var heroSettled = 0
    /** The banner's first picture has been shown (the loading screen waits for it). */
    var heroFirstPicture = false
    var continueCards by mutableStateOf(listOf<HomeTop.ContinueCard>())
        private set
    private val continueKnown = HashMap<Releases.Title, HomeTop.Known>()
    private val continueAsked = HashSet<Releases.Title>()
    var watchlist by mutableStateOf(listOf<Saved>())
        private set

    // When things come out

    private val releases = HashMap<Releases.Title, List<Release>>()
    private val calendarAsked = HashSet<Releases.Title>()
    private var calendarWaiting = 0
    private val calendarPictures = HashMap<Releases.Title, String>()
    var upcoming by mutableStateOf(listOf<Releases.Card>())
        private set
    /** Out since it went on the Watchlist (or after the last one watched)
     *  and not watched yet. */
    var watchNow by mutableStateOf(listOf<Releases.Card>())
        private set
    /** Every release known, by title (for the Calendar page, K16). */
    val allReleases: Map<Releases.Title, List<Release>> get() = releases

    /** Opens a title's page. */
    var open: (Opening) -> Unit = {}
    /** Notes a watched mark or Watchlist change for the linked accounts. */
    var queueSync: (what: String, kind: String, titleId: String, videoId: String) -> Unit = { _, _, _, _ -> }
    /** Every title watched, the latest film or episode of each, newest first. */
    var history by mutableStateOf(listOf<WatchProgress>())
        private set
    /** Goes up whenever something is marked watched or unwatched: views
     *  showing ticks read it, so they draw again. */
    var marks by mutableIntStateOf(0)
        private set

    /** Something was marked watched or unwatched: Continue Watching, the
     *  Watchlist and every tick on screen are put right. */
    fun marked() {
        marks += 1
        reloadContinue()
        reloadWatchlist()
        updateWatchNow()
    }

    /** Loads what was kept and starts filling Home, in the Rust build's order. */
    fun start() {
        loadSources()
        scope.launch { refreshBuiltin() }
        reloadHome()
        reloadContinue()
        reloadWatchlist()
        loadServers()
        startServerRefresh()
        reloadCatalog()
    }

    fun reloadHistory() {
        history = runCatching { store.history(300) }.getOrDefault(emptyList())
    }

    fun useKeys(keys: Secrets) {
        this.keys = keys
        reloadCatalog()
    }

    /** Home came into view or left it: its loading goes on or waits. */
    fun homeShowing(showing: Boolean) {
        homePause.set(!showing)
        if (showing) updateWatchNow()
    }

    // Sources

    fun loadSources() {
        addons = runCatching { InstalledAddons.load(store) }.getOrDefault(InstalledAddons())
    }

    /** Asks Cinemeta which catalogues it has now; Home is rebuilt if they changed. */
    suspend fun refreshBuiltin() {
        val fresh = runCatching { Cinemeta.refresh(store, builtin, catalogues) }.getOrNull() ?: return
        builtin = fresh
        reloadHome()
    }

    // Shelves

    /** Rebuilds Home's catalogue shelves: one per catalogue that needs no input. */
    fun reloadHome() {
        fill(homeBoard, installed.flatMap { source ->
            source.catalogs.filter { !it.needsInput }.map { ShelfPlan(it.title, source.name, ShelfFrom.Catalogue(source.url, it)) }
        })
    }

    /** Rebuilds the TMDB shelves on Home; none without a TMDB key. */
    fun reloadCatalog() {
        fill(catalogBoard, if (tmdb == null) emptyList() else TmdbShelves.plans.map {
            ShelfPlan(it.title, "TMDB", ShelfFrom.TmdbShelf(it.list, it.series))
        })
    }

    private val previews = ListSerializer(MetaPreview.serializer())

    /** Puts the planned shelves on a board, each loading, and fetches them.
     *  What a shelf held last time shows until the fresh list comes. */
    fun fill(board: ShelfBoard, plans: List<ShelfPlan>, search: String? = null) {
        val generation = board.generation.next()
        val keptLists = mutableListOf<Pair<Int, List<MetaPreview>>>()
        board.shelves = plans.mapIndexed { index, plan ->
            val name = if (board.board == Board.Search) "" else Kept.shelf(board.board.raw, plan.title, plan.source)
            if (name.isNotEmpty()) kept.read(name, previews)?.let { keptLists += index to it }
            Shelf(index, plan.title, plan.source, keptName = name)
        }
        val counted = board.board != Board.Search
        plans.forEachIndexed { index, plan ->
            if (counted) fetchesLeft += 1
            scope.launch {
                if (counted) homePause.pass()
                val result = titles(plan.from, search)
                if (counted) fetchesLeft = maxOf(fetchesLeft - 1, 0)
                loaded(board, generation, index, result, fromKept = false)
            }
        }
        for ((index, list) in keptLists) loaded(board, generation, index, Result.success(list), fromKept = true)
    }

    /** Fetches a shelf's titles. */
    private suspend fun titles(from: ShelfFrom, search: String?): Result<List<MetaPreview>> = runCatching {
        when (from) {
            is ShelfFrom.Catalogue -> catalogues.catalog(from.url, from.catalog, search)
            // A media server's shelves take turns.
            is ShelfFrom.OnServer -> serverGate.withPermit {
                if (from.latest) serverClient.latest(from.server, from.library)
                else serverClient.page(from.server, from.library, null, 0, ShelfLength.SERVER)
            }
            is ShelfFrom.ServerSearch -> serverClient.search(from.server, search ?: "")
            is ShelfFrom.TmdbShelf -> (tmdb ?: error("there is no TMDB key")).list(from.list, from.series, 1)
            is ShelfFrom.TmdbSearch -> (tmdb ?: error("there is no TMDB key")).search(from.series, search ?: "")
        }
    }

    /** Fills a shelf, from a fresh answer or (`fromKept`) from what it held
     *  last time. A fresh answer replaces the kept one, unless it failed or
     *  holds the same titles. */
    private fun loaded(board: ShelfBoard, generation: Long, index: Int, result: Result<List<MetaPreview>>, fromKept: Boolean) {
        if (!board.generation.isCurrent(generation) || index !in board.shelves.indices) return
        var shelf = board.shelves[index]
        val showingKept = !fromKept && !shelf.loading && shelf.items.isNotEmpty()
        shelf = shelf.copy(loading = false)
        result.fold(
            onSuccess = { metas ->
                // A server that gathers catalogues lists the same film more than once.
                val length = if (board.board == Board.Servers) ShelfLength.SERVER else ShelfLength.SHELF
                val list = Titles.withoutRepeats(metas).take(length)
                if (!fromKept && shelf.keptName.isNotEmpty()) kept.write(shelf.keptName, list, previews)
                shelf = shelf.copy(error = "")
                val same = showingKept && shelf.items.size == list.size &&
                    shelf.items.zip(list).all { (a, b) -> a.id == b.id && a.name == b.name }
                if (!same) {
                    if (board.board != Board.Search) offerHero(list)
                    shelf = shelf.copy(items = list)
                }
            },
            onFailure = { problem ->
                // The kept titles stay when the fresh ones cannot be had.
                if (!showingKept) shelf = shelf.copy(error = "Could not load this: ${words(problem)}.")
            },
        )
        board.shelves = board.shelves.toMutableList().also { it[index] = shelf }
    }

    private fun words(error: Throwable): String = error.cause?.let { Http.words(it) } ?: Http.words(error)

    // Search

    fun clearSearch() {
        fill(searchBoard, emptyList())
        searchNote = ""
    }

    /** Asks every searchable catalogue, every media server and TMDB for
     *  `query`; each answers as a shelf. */
    fun runSearch(query: String) {
        val words = query.trim()
        if (words.isEmpty()) return clearSearch()
        val plans = installed.flatMap { source ->
            source.catalogs.filter { it.searchable }.map { ShelfPlan(it.resultsTitle, source.name, ShelfFrom.Catalogue(source.url, it)) }
        } + servers.enabled.map { ShelfPlan("Results on ${it.name}", it.kind.label, ShelfFrom.ServerSearch(it)) } +
            (if (tmdb != null) listOf("Films" to false, "Series" to true).map { ShelfPlan(it.first, "TMDB", ShelfFrom.TmdbSearch(it.second)) } else emptyList())
        searchNote = searchNote(installed.size, plans.size, BuildConfig.ADDON_SCREENS)
        fill(searchBoard, plans, words)
    }

    // Media servers

    fun loadServers() {
        servers = ServerAccounts.load(store)
        serverStates = serverStates.filterKeys { id -> servers.enabled.any { it.id == id } }
        reloadServerShelves()
    }

    private var lastServerReload = 0L

    /** Keeps the servers' shelves fresh: loaded afresh when Haven comes back
     *  to the screen (a minute or more since the last time), every few
     *  minutes while it is open, and every minute while a server is offline. */
    private fun startServerRefresh() {
        scope.launch {
            while (true) {
                delay(60_000)
                val offline = serverStates.values.contains(ServerAccounts.State.Offline)
                if (offline || System.currentTimeMillis() - lastServerReload >= SERVER_RELOAD_EVERY_MILLIS) reloadServerShelves()
            }
        }
    }

    /** Haven came back to the screen. */
    fun cameBack() {
        if (System.currentTimeMillis() - lastServerReload >= 60_000) reloadServerShelves()
    }

    /** A server answered (its title's sources were asked): one marked
     *  offline is online again and gets its shelves back. */
    fun serverAnswered(id: Long) {
        if (serverStates[id] == ServerAccounts.State.Online) return
        val wasOffline = serverStates[id] == ServerAccounts.State.Offline
        serverStates = serverStates + (id to ServerAccounts.State.Online)
        if (wasOffline) reloadServerShelves()
    }

    private val libraryLists = ListSerializer(MediaLibrary.serializer())

    /** Asks every server for its libraries; each library then gets two
     *  shelves, the newest titles and the titles by name. The libraries found
     *  last time show at once. */
    fun reloadServerShelves() {
        lastServerReload = System.currentTimeMillis()
        val list = servers.enabled
        if (list.isEmpty()) {
            serverLibraries = emptyList()
            fill(serverBoard, emptyList())
            return
        }
        val keptLists = list.map { server -> server to (kept.read(Kept.libraries(server), libraryLists) ?: null) }
            .takeIf { all -> all.all { it.second != null } }?.map { it.first to it.second!! }
        var shown: List<String>? = null
        if (keptLists != null) {
            val plans = ServerAccounts.libraryPlans(keptLists)
            serverLibraries = keptLists.flatMap { (server, libraries) -> libraries.map { server to it } }
            shown = plans.map { "${it.title}|${it.source}" }
            fill(serverBoard, plans.map(::plan))
        }
        scope.launch {
            val listed = mutableListOf<Pair<Server, List<MediaLibrary>>>()
            for (server in list) {
                val libraries = withTimeoutOrNull(ServerAccounts.CHECK_WAIT_MILLIS) { runCatching { serverClient.libraries(server) }.getOrNull() }
                serverStates = serverStates + (server.id to if (libraries == null) ServerAccounts.State.Offline else ServerAccounts.State.Online)
                if (libraries == null) {
                    Log.note("could not load from ${server.name}")
                    continue
                }
                kept.write(Kept.libraries(server), libraries, libraryLists)
                listed += server to libraries
            }
            val plans = ServerAccounts.libraryPlans(listed)
            // The same shelves as the kept ones on screen: those stay.
            if (shown == plans.map { "${it.title}|${it.source}" }) return@launch
            serverLibraries = listed.flatMap { (server, libraries) -> libraries.map { server to it } }
            fill(serverBoard, plans.map(::plan))
        }
    }

    private fun plan(p: ServerAccounts.ShelfPlan) = ShelfPlan(p.title, p.source, ShelfFrom.OnServer(p.server, p.library, p.latest))

    // Details

    /** How to fetch a title's details from where it came from: its media
     *  server, TMDB, or the first source that knows it; null when nothing
     *  can. The answer is kept for the next start. */
    fun detailsOf(kind: String, id: String): (suspend () -> MetaDetail)? {
        val fetch: suspend () -> MetaDetail = ServerAccounts.serverItem(id, servers.enabled)?.let { (server, item) ->
            suspend { serverClient.meta(server, item) }
        } ?: Tmdb.splitId(id)?.let { (series, number) ->
            val tmdb = tmdb ?: return null
            suspend { tmdb.title(series, number) }
        } ?: run {
            val sources = installed.filter { it.provides("meta", kind, id) }.map { it.url }
            if (sources.isEmpty()) return null
            suspend {
                var last: Throwable = IllegalStateException("no source has it")
                var found: MetaDetail? = null
                for (url in sources) {
                    found = runCatching { catalogues.meta(url, kind, id) }.onFailure { last = it }.getOrNull()
                    if (found != null) break
                }
                found ?: throw last
            }
        }
        return {
            val meta = fetch()
            kept.write(Kept.details(kind, id), meta, MetaDetail.serializer())
            meta
        }
    }

    fun keptDetails(kind: String, id: String): MetaDetail? = kept.read(Kept.details(kind, id), MetaDetail.serializer())

    /** A film watched to the end (a series has no single tick). */
    fun filmWatched(kind: String, id: String): Boolean = kind == "movie" && finishedOf(kind, id).isNotEmpty()

    // Links to IMDb ids (as the services know titles)

    fun imdbLink(kind: String, titleId: String): ImdbLink? =
        if (Tmdb.isImdbId(titleId)) null else ImdbLink.of(titleId, keptDetails(kind, titleId))

    /** A title and video as the services know them (IMDb ids); null when
     *  there is none to go by. */
    fun serviceIds(kind: String, titleId: String, videoId: String): Pair<String, String>? =
        if (Tmdb.isImdbId(titleId)) titleId to videoId else imdbLink(kind, titleId)?.serviceIds(kind, videoId)

    /** A title's films or episodes watched to the end: its own records and
     *  those kept under its IMDb id (from a service). */
    fun finishedOf(kind: String, titleId: String): Set<String> {
        val own = runCatching { store.finishedVideos(kind, titleId) }.getOrDefault(emptyList()).toSet()
        val link = imdbLink(kind, titleId) ?: return own
        val elsewhere = runCatching { store.finishedVideos(kind, link.imdb) }.getOrDefault(emptyList()).toSet()
        return link.merged(kind, titleId, own, elsewhere)
    }

    /** Takes the watched marks an account no longer has off this device:
     *  each (kind, IMDb id, video) under its IMDb id, and the same film or
     *  episode of a media server's title linked to it (so it is not sent back
     *  as watched). Says, for each, whether anything was taken off. Runs on
     *  the tracker's thread. */
    fun forgetWatched(gone: List<Triple<String, String, String>>): List<Boolean> {
        if (gone.isEmpty()) return emptyList()
        val links = HashMap<String, ImdbLink?>()
        data class Linked(val row: WatchProgress, val imdb: String, val video: String)
        val linked = mutableListOf<Linked>()
        for (row in runCatching { store.allProgress() }.getOrDefault(emptyList())) {
            if (!row.finished || Tmdb.isImdbId(row.titleId)) continue
            val link = links.getOrPut(row.kind + "\u0001" + row.titleId) { imdbLink(row.kind, row.titleId) } ?: continue
            val (imdb, video) = link.serviceIds(row.kind, row.videoId) ?: continue
            linked += Linked(row, imdb, video)
        }
        return gone.map { (kind, titleId, videoId) ->
            var any = false
            val own = if (kind == "series") listOf(videoId) else runCatching { store.finishedVideos(kind, titleId) }.getOrDefault(emptyList())
            for (video in own) {
                if (runCatching { store.progress(kind, titleId, video)?.finished }.getOrNull() == true &&
                    runCatching { store.removeVideoProgress(kind, titleId, video) }.isSuccess
                ) any = true
            }
            for (entry in linked) {
                if (entry.row.kind == kind && entry.imdb == titleId && (kind != "series" || entry.video == videoId) &&
                    runCatching { store.removeVideoProgress(entry.row.kind, entry.row.titleId, entry.row.videoId) }.isSuccess
                ) any = true
            }
            any
        }
    }

    /** Queues what was watched on titles known by a media server's id that
     *  the accounts do not have yet. */
    fun sendMissingLinked() {
        val rows = runCatching { store.allProgress() }.getOrDefault(emptyList()).filter { it.finished && !Tmdb.isImdbId(it.titleId) }
        val queued = runCatching { store.queuedChanges() }.getOrDefault(emptyList())
        val links = HashMap<String, ImdbLink?>()
        for (row in rows) {
            val link = links.getOrPut(row.kind + "\u0001" + row.titleId) { imdbLink(row.kind, row.titleId) } ?: continue
            val (imdb, video) = link.serviceIds(row.kind, row.videoId) ?: continue
            val known = runCatching { store.progress(row.kind, imdb, video)?.finished }.getOrNull() == true
            val waiting = queued.any { it.titleId == imdb && it.videoId == video }
            if (!known && !waiting) queueSync("watched", row.kind, row.titleId, row.videoId)
        }
    }

    // The banner

    /** Fills the banner from the first shelf that loads with 3 titles or more. */
    private fun offerHero(metas: List<MetaPreview>) {
        if (heroStarted || metas.size < 3) return
        heroStarted = true
        val chosen = metas.take(HomeTop.SLIDES)
        hero = chosen.map { HomeTop.Slide(it.id, it.kind, it.name, it.releaseInfo ?: "") }
        chosen.forEachIndexed { slot, meta ->
            // Details kept from last time show at once; fresh ones follow.
            val keptMeta = keptDetails(meta.kind, meta.id)
            if (keptMeta != null) {
                heroSettled += 1
                heroDetails(slot, keptMeta)
            }
            val fetch = detailsOf(meta.kind, meta.id)
            if (fetch == null) {
                if (keptMeta == null) heroSettled += 1
                return@forEachIndexed
            }
            scope.launch {
                val found = runCatching { fetch() }.getOrNull()
                if (keptMeta == null) heroSettled += 1
                if (found != null && found != keptMeta) heroDetails(slot, found)
            }
        }
    }

    private fun setSlide(slot: Int, change: (HomeTop.Slide) -> HomeTop.Slide) {
        if (slot !in hero.indices) return
        hero = hero.toMutableList().also { it[slot] = change(it[slot]) }
    }

    private fun heroDetails(slot: Int, meta: MetaDetail) {
        setSlide(slot) {
            it.copy(
                facts = HomeTop.bannerFacts(meta, null),
                description = meta.description ?: "",
                scores = HomeTop.scorePills(meta, null, null),
                logo = meta.logo ?: it.logo,
                image = meta.background ?: meta.poster ?: it.image,
            )
        }
        heroScores(slot, meta)
    }

    /** Adds the TMDB, Rotten Tomatoes and Metacritic scores (and the age
     *  rating), where keys for them are set. OMDb's answers are kept a week. */
    private fun heroScores(slot: Int, meta: MetaDetail) {
        val imdb = meta.imdbId ?: meta.id.takeIf { Tmdb.isImdbId(it) } ?: return
        val tmdb = tmdb
        val omdbKey = keys.omdbApiKey
        val series = meta.kind == "series"
        val at = now()
        scope.launch {
            val rating = async { runCatching { tmdb?.details(series, imdb)?.rating }.getOrNull() }
            val scores = async { runCatching { kept.scores(imdb, omdbKey, at, http) }.getOrNull() }
            val (tmdbRating, omdb) = rating.await() to scores.await()
            val current = hero.getOrNull(slot) ?: return@launch
            if (current.id != meta.id && current.title != meta.name) return@launch
            setSlide(slot) { it.copy(facts = HomeTop.bannerFacts(meta, omdb), scores = HomeTop.scorePills(meta, tmdbRating, omdb)) }
        }
    }

    // Continue Watching

    private fun today() = Day.at(Instant.ofEpochSecond(now()), zone)

    /** Rebuilds Continue Watching from what was watched. */
    fun reloadContinue() {
        reloadHistory()
        val watched = runCatching { store.continueWatching(HomeTop.CONTINUE_LENGTH) }.getOrDefault(emptyList())
        // Details kept from last time, until the fresh ones come.
        for (p in watched) {
            val key = Releases.Title(p.kind, p.titleId)
            if (key !in continueKnown) keptDetails(key.kind, key.id)?.let { continueKnown[key] = HomeTop.Known(it.background, it.videos) }
        }
        continueCards = HomeTop.cards(watched, continueKnown, ::finished, today().toString())
        // Details (wide picture, episode list) once per title per run.
        for (p in watched) {
            val key = Releases.Title(p.kind, p.titleId)
            if (!continueAsked.add(key)) continue
            val fetch = detailsOf(key.kind, key.id) ?: continue
            scope.launch {
                val meta = runCatching { fetch() }.getOrNull() ?: return@launch
                val fresh = HomeTop.Known(meta.background, meta.videos)
                if (continueKnown[key] == fresh) return@launch
                continueKnown[key] = fresh
                val again = runCatching { store.continueWatching(HomeTop.CONTINUE_LENGTH) }.getOrDefault(emptyList())
                continueCards = HomeTop.cards(again, continueKnown, ::finished, today().toString())
            }
        }
    }

    private fun finished(p: WatchProgress) = finishedOf(p.kind, p.titleId)

    /** Takes a title off Continue Watching: forgets how far it (and, for a
     *  series, each of its episodes) was watched. */
    fun removeContinue(kind: String, id: String) {
        runCatching { store.removeProgress(kind, id) }.onFailure { Log.note("could not clear what was watched: ${it.message}") }
        reloadContinue()
    }

    // Watchlist

    /** Rebuilds the Watchlist from the database. Titles saved without a
     *  poster (brought in from Trakt or Simkl, say) are looked up, so their
     *  pictures show too; the poster found is saved with the title. */
    fun reloadWatchlist() {
        watchlist = runCatching { store.watchlist() }.getOrDefault(emptyList())
        watchlist.forEachIndexed { index, saved ->
            if (saved.poster != null) return@forEachIndexed
            keptDetails(saved.kind, saved.titleId)?.poster?.let {
                setPoster(index, saved, it)
                return@forEachIndexed
            }
            val fetch = detailsOf(saved.kind, saved.titleId)
            // An IMDb title nobody has details (or a poster) for yet: the
            // picture service Cinemeta itself uses.
            val fallback = if (Tmdb.isImdbId(saved.titleId)) "https://images.metahub.space/poster/medium/${saved.titleId}/img" else null
            if (fetch == null && fallback == null) return@forEachIndexed
            scope.launch {
                val found = fetch?.let { runCatching { it() }.getOrNull() }?.poster
                setPoster(index, saved, found ?: fallback ?: return@launch)
            }
        }
        refreshCalendar()
    }

    private fun setPoster(index: Int, saved: Saved, poster: String) {
        val updated = saved.copy(poster = poster)
        runCatching { store.addToWatchlist(updated) }
        val now = watchlist.getOrNull(index) ?: return
        if (now.titleId == saved.titleId && now.kind == saved.kind) watchlist = watchlist.toMutableList().also { it[index] = now.copy(poster = poster) }
    }

    // When things come out

    /** Brings the release dates in line with the Watchlist: asks for the
     *  details of titles not asked for yet (2 at a time), forgets titles taken
     *  off. Home's Upcoming row is made from them. */
    fun refreshCalendar() {
        val listed = watchlist.map { Releases.Title(it.kind, it.titleId) }.toSet()
        releases.keys.retainAll(listed)
        calendarAsked.retainAll(listed)
        for (title in listed) {
            if (!calendarAsked.add(title)) continue
            // The dates found last time show at once; fresh ones follow.
            val keptMeta = keptDetails(title.kind, title.id)
            if (keptMeta != null) {
                (keptMeta.background ?: keptMeta.poster)?.let { calendarPictures[title] = it }
                releases[title] = Releases.of(keptMeta, zone)
            }
            val fetch = detailsOf(title.kind, title.id) ?: continue
            val counted = keptMeta == null
            if (counted) calendarWaiting += 1
            fetchesLeft += 1
            scope.launch {
                val found = calendarGate.withPermit { runCatching { fetch() }.getOrNull() }
                if (counted) calendarWaiting = maxOf(calendarWaiting - 1, 0)
                fetchesLeft = maxOf(fetchesLeft - 1, 0)
                // Taken off the Watchlist meanwhile: not wanted.
                if (title !in calendarAsked) return@launch
                if (found != null) {
                    (found.background ?: found.poster)?.let { calendarPictures[title] = it }
                    releases[title] = Releases.of(found, zone)
                } else {
                    // Asked again next time (the kept dates stay meanwhile).
                    Log.note("calendar: no dates for a title")
                    calendarAsked.remove(title)
                }
                updateUpcoming()
            }
        }
        updateUpcoming()
    }

    /** Asks every title's dates again (Home pulled down). */
    fun recheckCalendar() {
        calendarAsked.clear()
        refreshCalendar()
    }

    private fun names() = watchlist.associate { Releases.Title(it.kind, it.titleId) to it.title }

    private fun updateUpcoming() {
        upcoming = Releases.upcoming(releases, names(), calendarPictures, today(), Instant.ofEpochSecond(now()), zone)
        updateWatchNow()
    }

    /** Fills Watch now again (also when Home shows, so what was just watched
     *  leaves it). */
    fun updateWatchNow() {
        val added = watchlist.associate { Releases.Title(it.kind, it.titleId) to Day.at(Instant.ofEpochMilli(it.addedAt), zone) }
        val done = releases.keys.associateWith { finishedOf(it.kind, it.id) }
        val cards = Releases.watchNow(releases, names(), calendarPictures, added, { title, release ->
            val seen = done[title] ?: return@watchNow false
            if (release.video.isEmpty()) seen.isNotEmpty() else release.video in seen
        }, today())
        if (cards != watchNow) watchNow = cards
    }

    // Pull to refresh

    /** Loads Home afresh: every shelf, the servers' libraries, the dates and
     *  Continue Watching. What is on screen stays until each fresh answer
     *  replaces it. Returns once nothing is still out (at most 20 s). */
    suspend fun refreshHome() {
        Log.note("home: loading afresh (pulled down)")
        refreshing = true
        reloadHome()
        reloadCatalog()
        reloadServerShelves()
        recheckCalendar()
        continueAsked.clear()
        reloadContinue()
        val started = System.currentTimeMillis()
        while (fetchesLeft > 0 && System.currentTimeMillis() - started < 20_000) delay(250)
        refreshing = false
    }

    // Loading screen

    /** How far Haven has got with what it loads at start, 0 to 1: Home's
     *  shelves, the banner's details and its first picture, and the dates. A
     *  failed request counts as done, so the screen never waits for an answer
     *  that will not come. */
    fun startupProgress(homeShowing: Boolean): Double {
        val parts = mutableListOf<Double>()
        if (homeShowing) {
            val boards = listOf(homeBoard, serverBoard, catalogBoard)
            val total = boards.sumOf { it.shelves.size }
            val done = boards.sumOf { it.settled }
            val hasSources = installed.isNotEmpty() || servers.enabled.isNotEmpty()
            parts += if (total == 0) (if (hasSources) 0.0 else 1.0) else done.toDouble() / total
            if (heroStarted && hero.isNotEmpty()) {
                parts += minOf(heroSettled, hero.size).toDouble() / hero.size * 0.7 + if (heroFirstPicture) 0.3 else 0.0
            } else if (total > 0 && done < total) {
                parts += 0.0
            }
        }
        if (calendarAsked.isNotEmpty()) {
            parts += (calendarAsked.size - minOf(calendarWaiting, calendarAsked.size)).toDouble() / calendarAsked.size
        }
        return if (parts.isEmpty()) 1.0 else parts.sum() / parts.size
    }

    /** Home has something to show (else it explains how to fill it). */
    val homeHasSources get() = homeBoard.shelves.isNotEmpty() || servers.enabled.isNotEmpty() || catalogBoard.shelves.isNotEmpty()

    companion object {
        /** How often the servers' shelves are loaded afresh while Haven is open. */
        const val SERVER_RELOAD_EVERY_MILLIS = 5 * 60_000L

        fun searchNote(sources: Int, plans: Int, addons: Boolean): String = when {
            plans != 0 -> ""
            addons && sources == 0 -> "Add a media server or a TMDB key in Settings, or install an add-on: search looks through those."
            addons -> "None of your add-ons can be searched."
            sources == 0 -> "Add a media server or a TMDB key in Settings: search looks through those."
            else -> "Nothing you have added can be searched."
        }

        /** Home with nothing to show. */
        fun emptyText(addons: Boolean) = if (addons) {
            "Home fills with shelves of films and series from your media servers, your add-ons and the TMDB catalogue. Add a media server or a TMDB key in Settings, or install an add-on."
        } else {
            "Home fills with shelves of films and series from your media servers and the TMDB catalogue. Add a media server or a TMDB key in Settings."
        }
    }
}
