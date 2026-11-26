package net.haven.app.app

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableDoubleStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import net.haven.app.core.AppPaths
import net.haven.app.core.Log
import net.haven.app.core.Settings
import net.haven.app.core.Store
import net.haven.app.core.Theme
import net.haven.app.core.Keys
import net.haven.app.core.Secrets
import net.haven.app.library.Library
import net.haven.app.library.Opening
import net.haven.app.pictures.Cache
import net.haven.app.pictures.Kept
import net.haven.app.tracking.Tracker

/** What Haven opens at start: the data folder, its database, the service
 *  keys and the cache. `existing` is false on a fresh install (no settings
 *  saved yet). */
class Core(
    val paths: AppPaths,
    val store: Store,
    val existing: Boolean,
    val secrets: Secrets,
    val kept: Kept,
    /** Where downloads go (Movies › Haven, else Haven's own folder). */
    val downloads: java.io.File = java.io.File(paths.dataDir, "Downloads"),
) {
    companion object {
        fun open(paths: AppPaths, cacheDir: java.io.File, movies: java.io.File? = null): Core {
            val store = Store.open(paths.database)
            val existing = store.rawValue(Settings.KEY) != null
            val secrets = runCatching { Keys.load(store) }.getOrDefault(Keys()).resolved(Secrets.fromFile(paths.secretsFile))
            return Core(paths, store, existing, secrets, Cache.kept(cacheDir), net.haven.app.library.DownloadRules.root(movies, paths.dataDir))
        }
    }
}

/** The whole app's state while it runs: which page shows, the theme, the
 *  loading screen, What's New, and (from K10 on) the parts that work behind
 *  the pages. Lives as long as the process, so turning the phone keeps it. */
class AppModel(val core: Core, val notes: WhatsNew.Notes?) {
    val store get() = core.store
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    var settings by mutableStateOf(runCatching { Settings.load(core.store) }.getOrElse { Settings() })
        private set
    /** The page showing. A title's page (K12) lies over it. */
    var page by mutableStateOf(Page.of(settings.lastPage))
        private set
    /** A title's page is open over the pages (no tab is then highlighted). */
    var detailOpen by mutableStateOf(false)
        private set
    /** The title whose page is open (its page comes in K12). */
    var opening by mutableStateOf<Opening?>(null)
        private set
    /** The Library's part showing: 0 Local, 1 Downloads, 2 Watchlist, 3 History. */
    var libraryTab by mutableStateOf(0)

    val tracker = Tracker(core.store, core.secrets)
    val library = Library(core.store, core.secrets, kept = core.kept, downloadsRoot = core.downloads,
        downloadPictures = java.io.File(core.paths.dataDir, "download-pictures"))
    /** The Calendar page's month or week and the day picked. */
    val calendar = net.haven.app.library.CalendarModel(library)
    /** Haven's own player (libmpv); made by the app with the engine for this device. */
    lateinit var player: net.haven.app.player.PlayerModel
        private set
    /** Chromecasts. */
    lateinit var cast: net.haven.app.cast.CastModel
        private set
    /** Live TV and radio. */
    val live = net.haven.app.livetv.LiveTVModel(core.store, scope)
    /** Settings' parts: what each shows and does. */
    val settingsPage = SettingsModel(core.store, core.paths, library, tracker, live, scope, net.haven.app.BuildConfig.ADDON_SCREENS)
    /** A radio station started (true) or stopped (false): it keeps playing in
     *  the background meanwhile. */
    var radio: (Boolean) -> Unit = {}

    /** Hands a video to a player app on the phone; false if none could take it. */
    var openElsewhere: (net.haven.app.library.PlayRequest) -> Boolean = { false }
    /** Settings was tapped again while showing: back to its list (K17). */
    var settingsBackToList by mutableStateOf(0)
        private set

    // The loading screen.
    var splashProgress by mutableDoubleStateOf(0.0)
        private set
    var splashShown by mutableStateOf(true)
        private set
    var splashGone by mutableStateOf(false)
        private set
    private val whatsNewDue = WhatsNew.due(notes, core.store, core.existing)
    var whatsNewShown by mutableStateOf(false)
        private set

    /** How far start-up has got, 0 to 1 (Home's shelves feed it from K10;
     *  until then there is nothing to wait for). */
    var startupProgress: () -> Double = { 1.0 }

    private var started = false

    init {
        tracker.watchlistChanged = { scope.launch { library.reloadWatchlist() } }
        // Marks brought in from the accounts.
        tracker.watchedChanged = { scope.launch { library.marked() } }
        // The services go by IMDb id: a title known by a media server's id is
        // sent under its IMDb id (one with none is not sent).
        library.queueSync = { what, kind, titleId, videoId ->
            library.serviceIds(kind, titleId, videoId)?.let { (title, video) -> tracker.queue(what, kind, title, video) }
        }
        tracker.pulledAll = { scope.launch { library.sendMissingLinked() } }
        // Marks taken off on another device go from linked server titles too.
        tracker.forgetWatched = { gone -> library.forgetWatched(gone) }
        library.open = { open(it) }
        // Settings shows the sign-ins and the sync as they change.
        tracker.changed = { settingsPage.refresh() }
        tracker.openLink = { settingsPage.openLink(it) }
        settingsPage.player = { if (this::player.isInitialized) player else null }
    }

    /** Gives the app its player and Chromecast link (they need Android). */
    fun attach(player: net.haven.app.player.PlayerModel, cast: net.haven.app.cast.CastModel) {
        if (this::player.isInitialized) return
        this.player = player
        this.cast = cast
        player.closed = { played ->
            radio(false)
            library.playerClosed(played)
            // The watched mark goes out and other devices' changes come in.
            tracker.watchedSomething()
        }
        player.next = { library.playNext(it) }
        library.dismissPlayer = { player.dismiss() }
        cast.closed = { played ->
            library.playerClosed(played)
            tracker.watchedSomething()
        }
        library.play = { play(it) }
    }

    /** Plays a film or episode: in the player app chosen in Settings, on the
     *  Chromecast chosen under the places to watch, else in Haven's player. */
    fun play(request: net.haven.app.library.PlayRequest) {
        val page = library.title
        val fromTitle = request.titleId.isNotEmpty()
        val other = net.haven.app.player.PlayerRules.OtherPlayer.load(store)
        if (net.haven.app.player.PlayerRules.goesElsewhere(other, request.address)) {
            // Another app does not say how far it got: no resuming with it.
            // (Radio and TV channels too go to it.)
            val opened = openElsewhere(request)
            if (fromTitle) page.playNote = if (opened) "Opened in your player." else "No player app could open this. Install one (VLC, say), or choose Haven's own player in Settings."
            return
        }
        if (cast.chosen != null) {
            val why = cast.whyNot(request)
            if (why == null) {
                cast.cast(request)
                if (fromTitle) page.playNote = "Playing on ${cast.deviceName}."
                return
            }
            // The Chromecast cannot have this one: it plays here.
            if (fromTitle) page.playNote = "Playing here: $why."
        }
        player.play(request)
        radio(request.kind == "radio")
    }

    /** Starts what works behind the pages, in the Rust build's order. */
    fun start() {
        if (started) return
        started = true
        library.homeShowing(page == Page.Home)
        library.start()
        startupProgress = { library.startupProgress(homeShowing = page == Page.Home && !detailOpen) }
        tracker.start()
        live.play = { play(it) }
        live.start()
        settingsPage.stremioRefresh()
        settingsPage.startBackups()
        runSplash()
    }

    /** Haven came back to the screen. */
    fun cameBack() {
        if (!started) return
        tracker.cameBack()
        library.cameBack()
    }

    /** Opens a title's page. */
    fun open(title: Opening) {
        opening = title
        detailOpen = true
        library.homeShowing(false)
        library.title.open(title)
    }

    fun closeTitle() {
        library.title.close()
        opening = null
        detailOpen = false
        library.homeShowing(page == Page.Home)
    }

    /** Opens the Library on one of its parts (Home's row arrows). */
    fun openLibrary(tab: Int) {
        libraryTab = tab
        select(Page.Library)
    }

    /** Home came into view or left it. */
    fun homeAppeared(shown: Boolean) {
        library.homeShowing(shown && !detailOpen)
    }

    // Pages

    fun select(new: Page) {
        if (new == Page.Settings && page == Page.Settings && !detailOpen) settingsBackToList += 1
        if (opening != null) library.title.close()
        opening = null
        detailOpen = false
        page = new
        library.homeShowing(new == Page.Home)
        settings = settings.copy(lastPage = new.ordinal)
        save()
    }

    // Theme

    val dark get() = settings.theme == Theme.Dark

    fun setDark(dark: Boolean) {
        settings = settings.copy(theme = if (dark) Theme.Dark else Theme.Light)
        save()
    }

    private fun save() {
        runCatching { settings.save(core.store) }.onFailure { Log.note("could not save the settings: ${it.message}") }
    }

    // Loading screen

    companion object {
        /** The loading screen shows for at least a second (so it does not
         *  flash), else until start-up is done, and never more than 12
         *  seconds, so a slow or dead network never keeps Haven shut. */
        const val SPLASH_LONGEST = 12.0
        const val SPLASH_TICK_MILLIS = 80L
        const val SPLASH_FADE_MILLIS = 400L

        /** The bar's place: in the first second it cannot run ahead of the clock. */
        fun splashTarget(real: Double, elapsed: Double): Double {
            val r = if (elapsed >= SPLASH_LONGEST) 1.0 else real.coerceIn(0.0, 1.0)
            return if (elapsed < 1) minOf(r, elapsed) else r
        }

        /** "Loading…  NN%", then "Ready". */
        fun splashText(progress: Double) =
            if (progress >= 1) "Ready" else "Loading…  ${Math.round(progress.coerceIn(0.0, 1.0) * 100)}%"
    }

    private fun runSplash() {
        val startedAt = System.nanoTime()
        scope.launch {
            while (true) {
                delay(SPLASH_TICK_MILLIS)
                val elapsed = (System.nanoTime() - startedAt) / 1e9
                splashProgress = maxOf(splashProgress, splashTarget(runCatching { startupProgress() }.getOrDefault(1.0), elapsed))
                if (splashProgress >= 1 && elapsed >= 1) {
                    Log.note("loading screen closed after ${"%.1f".format(java.util.Locale.ROOT, elapsed)} s")
                    splashShown = false
                    delay(SPLASH_FADE_MILLIS + 50)
                    splashGone = true
                    if (whatsNewDue) whatsNewShown = true
                    return@launch
                }
            }
        }
    }

    // What's New

    fun whatsNewDone(dontShowAgain: Boolean) {
        WhatsNew.done(notes, core.store, dontShowAgain)
        whatsNewShown = false
    }
}
