package net.haven.app

import android.app.UiModeManager
import android.content.res.Configuration
import android.graphics.Color
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import net.haven.app.app.AppModel
import net.haven.app.app.Core
import net.haven.app.app.WhatsNew
import net.haven.app.core.AppPaths
import net.haven.app.core.Log
import net.haven.app.pictures.Cache
import net.haven.app.ui.CouldNotStart
import net.haven.app.ui.HavenTheme
import net.haven.app.ui.LocalPictures
import net.haven.app.ui.Shell

/** Haven's one window. The app's state lives as long as the process (see
 *  [HavenApp.model]), so turning the phone or resizing keeps everything. */
class MainActivity : ComponentActivity() {
    private var tv = false
    /** The player shows in a small window over other apps. */
    private val pictureInPicture = androidx.compose.runtime.mutableStateOf(false)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        tv = (getSystemService(UI_MODE_SERVICE) as UiModeManager).currentModeType == Configuration.UI_MODE_TYPE_TELEVISION
        val started = HavenApp.model(this)
        started.getOrNull()?.let { model ->
            model.attach(
                net.haven.app.player.PlayerModel(model.store, HavenApp.engine(this), kotlinx.coroutines.MainScope(), tv),
                net.haven.app.cast.CastModel(model.store) { onEvent ->
                    if (tv) null else runCatching { net.haven.app.cast.Caster(applicationContext, onEvent) }.getOrNull()
                },
            )
            model.openElsewhere = { request -> openElsewhere(request.address, request.title) }
            // Debug builds only: `adb shell am start -n net.haven.app/.MainActivity
            // --es haven.play <address>` plays an address, to try the player.
            if (BuildConfig.DEBUG) intent?.getStringExtra("haven.play")?.let { address ->
                window.decorView.postDelayed({
                    model.player.play(net.haven.app.library.PlayRequest(address, "Player check", "", "movie", "debug:play", "debug:play", null, "adb", remember = false))
                }, 4_000)
            }
        }
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
                    CompositionLocalProvider(LocalPictures provides HavenApp.pictures(this)) { Shell(model, pictureInPicture.value) }
                }
            }
        }
    }

    override fun onResume() {
        super.onResume()
        HavenApp.model(this).getOrNull()?.cameBack()
    }

    /** Leaving Haven while a film plays: it goes on in a small window (not on
     *  a TV, nor while paused). */
    override fun onUserLeaveHint() {
        super.onUserLeaveHint()
        val player = HavenApp.model(this).getOrNull()?.player ?: return
        if (tv || !player.open || player.paused || player.error.isNotEmpty() || player.isRadio) return
        runCatching {
            enterPictureInPictureMode(android.app.PictureInPictureParams.Builder().setAspectRatio(android.util.Rational(16, 9)).build())
        }
    }

    override fun onPictureInPictureModeChanged(inPicture: Boolean, newConfig: Configuration) {
        super.onPictureInPictureModeChanged(inPicture, newConfig)
        pictureInPicture.value = inPicture
    }

    /** Haven went away (not into a small window): the film waits, its place kept. */
    override fun onStop() {
        super.onStop()
        if (!isInPictureInPictureMode) HavenApp.model(this).getOrNull()?.player?.leaving()
    }

    /** Hands a video to a player app: Android lists those installed (VLC, mpv
     *  and so on). False if none can take it. */
    private fun openElsewhere(address: String, title: String): Boolean = runCatching {
        val view = android.content.Intent(android.content.Intent.ACTION_VIEW)
            .setDataAndType(android.net.Uri.parse(address), "video/*")
            .putExtra("title", title)
            .putExtra(android.content.Intent.EXTRA_TITLE, title)
        startActivity(android.content.Intent.createChooser(view, "Play with"))
    }.isSuccess
}

/** Opens Haven's data once per process. */
object HavenApp {
    private var started: Result<AppModel>? = null
    private var player: net.haven.app.player.MpvEngine? = null

    /** The one libmpv for the whole run. */
    @Synchronized
    fun engine(activity: ComponentActivity) = player ?: net.haven.app.player.MpvEngine(activity.applicationContext).also { player = it }
    private var shared: net.haven.app.pictures.Pictures<android.graphics.Bitmap>? = null

    /** The picture cache, in Android's cache folder for the app. */
    @Synchronized
    fun pictures(activity: ComponentActivity) = shared ?: net.haven.app.pictures.Pictures(
        Cache.pictures(activity.cacheDir), net.haven.app.pictures.AndroidPictures,
    ).also { shared = it }

    @Synchronized
    fun model(activity: ComponentActivity): Result<AppModel> = started ?: runCatching {
        val paths = AppPaths.of(activity)
        Log.start(paths.logFile)
        Log.note("Haven ${BuildConfig.VERSION_NAME} build ${BuildConfig.HAVEN_BUILD} (Kotlin) starting; libmpv ${Mpv.apiVersion()?.let { "${it shr 16}.${it and 0xffff}" } ?: "missing"}")
        Cache.removeRustCache(paths)
        val notes = runCatching { activity.assets.open("whats-new.json").bufferedReader().use { it.readText() } }
            .getOrNull()?.let(WhatsNew::parse)
        AppModel(Core.open(paths, activity.cacheDir), notes)
    }.onFailure { Log.note("could not start: ${it.message}") }.also { started = it }
}
