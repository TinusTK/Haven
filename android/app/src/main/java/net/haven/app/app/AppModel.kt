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

/** What Haven opens at start: the data folder and its database. `existing`
 *  is false on a fresh install (no settings saved yet). */
class Core(val paths: AppPaths, val store: Store, val existing: Boolean) {
    companion object {
        fun open(paths: AppPaths): Core {
            val store = Store.open(paths.database)
            val existing = store.rawValue(Settings.KEY) != null
            return Core(paths, store, existing)
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

    fun start() {
        if (started) return
        started = true
        runSplash()
    }

    // Pages

    fun select(new: Page) {
        if (new == Page.Settings && page == Page.Settings && !detailOpen) settingsBackToList += 1
        detailOpen = false
        page = new
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
