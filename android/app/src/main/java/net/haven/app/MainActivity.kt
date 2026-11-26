package net.haven.app

import android.app.UiModeManager
import android.content.res.Configuration
import android.graphics.Color
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.LaunchedEffect
import net.haven.app.app.AppModel
import net.haven.app.app.Core
import net.haven.app.app.WhatsNew
import net.haven.app.core.AppPaths
import net.haven.app.core.Log
import net.haven.app.pictures.Cache
import net.haven.app.ui.CouldNotStart
import net.haven.app.ui.HavenTheme
import net.haven.app.ui.Shell

/** Haven's one window. The app's state lives as long as the process (see
 *  [HavenApp.model]), so turning the phone or resizing keeps everything. */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val tv = (getSystemService(UI_MODE_SERVICE) as UiModeManager).currentModeType == Configuration.UI_MODE_TYPE_TELEVISION
        val started = HavenApp.model(this)
        setContent {
            val model = started.getOrNull()
            val dark = model?.dark ?: true
            LaunchedEffect(dark) {
                val bars = if (dark) SystemBarStyle.dark(Color.TRANSPARENT) else SystemBarStyle.light(Color.TRANSPARENT, Color.TRANSPARENT)
                enableEdgeToEdge(bars, bars)
            }
            HavenTheme(dark = dark, tv = tv) {
                if (model == null) {
                    CouldNotStart(started.exceptionOrNull()?.message ?: "unknown error")
                } else {
                    LaunchedEffect(Unit) { model.start() }
                    Shell(model)
                }
            }
        }
    }
}

/** Opens Haven's data once per process. */
object HavenApp {
    private var started: Result<AppModel>? = null

    @Synchronized
    fun model(activity: ComponentActivity): Result<AppModel> = started ?: runCatching {
        val paths = AppPaths.of(activity)
        Log.start(paths.logFile)
        Log.note("Haven ${BuildConfig.VERSION_NAME} build ${BuildConfig.HAVEN_BUILD} (Kotlin) starting; libmpv ${Mpv.apiVersion()?.let { "${it shr 16}.${it and 0xffff}" } ?: "missing"}")
        Cache.removeRustCache(paths)
        val notes = runCatching { activity.assets.open("whats-new.json").bufferedReader().use { it.readText() } }
            .getOrNull()?.let(WhatsNew::parse)
        AppModel(Core.open(paths), notes)
    }.onFailure { Log.note("could not start: ${it.message}") }.also { started = it }
}
