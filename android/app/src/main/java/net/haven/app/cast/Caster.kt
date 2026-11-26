package net.haven.app.cast

import android.content.Context
import android.net.Uri
import android.os.Handler
import android.os.Looper
import androidx.mediarouter.media.MediaRouteSelector
import androidx.mediarouter.media.MediaRouter
import com.google.android.gms.cast.CastMediaControlIntent
import com.google.android.gms.cast.MediaInfo
import com.google.android.gms.cast.MediaLoadRequestData
import com.google.android.gms.cast.MediaMetadata
import com.google.android.gms.cast.MediaSeekOptions
import com.google.android.gms.cast.MediaStatus
import com.google.android.gms.cast.framework.CastContext
import com.google.android.gms.cast.framework.CastSession
import com.google.android.gms.cast.framework.OptionsProvider
import com.google.android.gms.cast.framework.SessionManagerListener
import com.google.android.gms.cast.framework.SessionProvider
import com.google.android.gms.cast.framework.media.RemoteMediaClient
import com.google.android.gms.common.images.WebImage
import net.haven.app.core.Log
import com.google.android.gms.cast.framework.CastOptions as SdkCastOptions

/** Tells the Cast SDK which app plays on the Chromecast: Google's default
 *  media receiver, which fetches the address itself (named in the manifest). */
class CastOptions : OptionsProvider {
    override fun getCastOptions(context: Context): SdkCastOptions =
        SdkCastOptions.Builder()
            .setReceiverApplicationId(CastMediaControlIntent.DEFAULT_MEDIA_RECEIVER_APPLICATION_ID)
            .setStopReceiverApplicationWhenEndingSession(true)
            .build()

    override fun getAdditionalSessionProviders(context: Context): List<SessionProvider>? = null
}

/**
 * Casting a stream to a Chromecast instead of playing it here: finding the
 * devices on the network (MediaRouter), connecting (the Cast SDK), and
 * playing, pausing, skipping and stopping. Everything runs on the main
 * thread; `onEvent` is called there.
 *
 * Needs Google Play services; without them (some TVs, de-Googled phones)
 * [available] is false and Haven only plays on the device.
 */
class Caster(context: Context, private val onEvent: (CastEvent) -> Unit) {
    companion object {
        /** How often the Chromecast is asked where playback stands. */
        const val TICK_MILLIS = 2_000L
        const val BACK_SECONDS = -10.0
        const val FORWARD_SECONDS = 30.0
    }

    private val app = context.applicationContext
    private val main = Handler(Looper.getMainLooper())
    private val cast: CastContext? = runCatching { CastContext.getSharedInstance(app) }
        .onFailure { Log.note("casting is not available here: ${it.message}") }
        .getOrNull()

    /** Whether this device can cast at all. */
    val available get() = cast != null

    private val router by lazy { MediaRouter.getInstance(app) }
    private val selector = MediaRouteSelector.Builder()
        .addControlCategory(CastMediaControlIntent.categoryForCast(CastMediaControlIntent.DEFAULT_MEDIA_RECEIVER_APPLICATION_ID))
        .build()

    /** What waits for the device to connect; played once it has. */
    private var waiting: CastRequest? = null
    private var playing: RemoteMediaClient? = null
    private var lastPosition = 0.0
    private var lastDuration = 0.0

    // Finding devices

    private val scan = object : MediaRouter.Callback() {}

    /** Listens for Chromecasts for `millis`, then hands back those found, by
     *  name. */
    fun find(millis: Long = 4_000, found: (List<CastDevice>) -> Unit) {
        if (cast == null) return found(emptyList())
        router.addCallback(selector, scan, MediaRouter.CALLBACK_FLAG_PERFORM_ACTIVE_SCAN)
        main.postDelayed({
            router.removeCallback(scan)
            found(devices())
        }, millis)
    }

    /** The Chromecasts known right now. */
    fun devices(): List<CastDevice> =
        if (cast == null) emptyList()
        else router.routes
            .filter { !it.isDefault && it.isEnabled && it.matchesSelector(selector) }
            .map { CastDevice(it.id, it.name) }
            .distinctBy { it.id }
            .sortedBy { it.name.lowercase() }

    // Playing

    /** Plays `request` on `device`: connects first if it is not the device
     *  already in use. */
    fun play(device: CastDevice, request: CastRequest) {
        val cast = cast ?: return onEvent(CastEvent.Failed("casting is not available on this device"))
        CastRules.whyNot(request.address)?.let { return onEvent(CastEvent.Failed(it)) }
        onEvent(CastEvent.Connecting(device.name))
        val session = cast.sessionManager.currentCastSession
        val same = session?.castDevice?.let { (it.deviceId.isNotEmpty() && device.id.contains(it.deviceId)) || it.friendlyName == device.name } == true
        if (session?.isConnected == true && same) {
            load(session, request)
            return
        }
        val route = router.routes.firstOrNull { it.id == device.id }
            ?: return onEvent(CastEvent.Failed("${device.name} is no longer on the network"))
        waiting = request
        router.selectRoute(route)
    }

    private val sessions = object : SessionManagerListener<CastSession> {
        override fun onSessionStarted(session: CastSession, sessionId: String) = connected(session)
        override fun onSessionResumed(session: CastSession, wasSuspended: Boolean) = connected(session)
        override fun onSessionStartFailed(session: CastSession, error: Int) = failed("could not connect to the Chromecast (error $error)")
        override fun onSessionResumeFailed(session: CastSession, error: Int) = failed("could not connect to the Chromecast (error $error)")
        override fun onSessionEnded(session: CastSession, error: Int) = ended()
        override fun onSessionSuspended(session: CastSession, reason: Int) {}
        override fun onSessionStarting(session: CastSession) {}
        override fun onSessionEnding(session: CastSession) {}
        override fun onSessionResuming(session: CastSession, sessionId: String) {}
    }

    init {
        cast?.sessionManager?.addSessionManagerListener(sessions, CastSession::class.java)
    }

    private fun connected(session: CastSession) {
        val request = waiting ?: return
        waiting = null
        load(session, request)
    }

    private fun failed(reason: String) {
        waiting = null
        detach()
        onEvent(CastEvent.Failed(reason))
    }

    private fun ended() {
        waiting = null
        if (detach()) onEvent(CastEvent.Ended)
    }

    private fun load(session: CastSession, request: CastRequest) {
        val client = session.remoteMediaClient ?: return failed("the Chromecast did not answer")
        detach()
        val metadata = MediaMetadata(MediaMetadata.MEDIA_TYPE_MOVIE).apply {
            putString(MediaMetadata.KEY_TITLE, request.title)
            if (request.detail.isNotEmpty()) putString(MediaMetadata.KEY_SUBTITLE, request.detail)
            request.poster?.let { addImage(WebImage(Uri.parse(it))) }
        }
        val media = MediaInfo.Builder(request.address)
            .setContentUrl(request.address)
            .setStreamType(MediaInfo.STREAM_TYPE_BUFFERED)
            .setContentType(CastRules.contentType(request.address))
            .setMetadata(metadata)
            .build()
        val load = MediaLoadRequestData.Builder()
            .setMediaInfo(media)
            .setAutoplay(true)
            .setCurrentTime((request.start * 1000).toLong())
            .build()
        playing = client
        lastPosition = request.start
        lastDuration = 0.0
        client.registerCallback(status)
        client.addProgressListener(progress, TICK_MILLIS)
        client.load(load).setResultCallback { result ->
            if (playing !== client) return@setResultCallback
            if (result.status.isSuccess) onEvent(CastEvent.Started)
            else failed("the Chromecast could not play this stream (${result.status.statusCode})")
        }
    }

    /** Stops listening to the stream that was playing; true if one was. */
    private fun detach(): Boolean {
        val client = playing ?: return false
        playing = null
        client.unregisterCallback(status)
        client.removeProgressListener(progress)
        return true
    }

    private val progress = RemoteMediaClient.ProgressListener { position, duration ->
        lastPosition = position / 1000.0
        lastDuration = maxOf(duration, 0L) / 1000.0
        onEvent(CastEvent.Position(lastPosition, lastDuration, playing?.isPaused == true))
    }

    private val status = object : RemoteMediaClient.Callback() {
        override fun onStatusUpdated() {
            val client = playing ?: return
            val state = when (client.playerState) {
                MediaStatus.PLAYER_STATE_IDLE -> CastRules.PlayerState.Idle
                MediaStatus.PLAYER_STATE_BUFFERING -> CastRules.PlayerState.Buffering
                MediaStatus.PLAYER_STATE_PLAYING -> CastRules.PlayerState.Playing
                MediaStatus.PLAYER_STATE_PAUSED -> CastRules.PlayerState.Paused
                MediaStatus.PLAYER_STATE_LOADING -> CastRules.PlayerState.Loading
                else -> CastRules.PlayerState.Unknown
            }
            val reason = when (client.idleReason) {
                MediaStatus.IDLE_REASON_FINISHED -> CastRules.IdleReason.Finished
                MediaStatus.IDLE_REASON_CANCELED -> CastRules.IdleReason.Cancelled
                MediaStatus.IDLE_REASON_INTERRUPTED -> CastRules.IdleReason.Interrupted
                MediaStatus.IDLE_REASON_ERROR -> CastRules.IdleReason.Error
                else -> CastRules.IdleReason.None
            }
            when (val end = CastRules.ending(state, reason)) {
                null -> onEvent(CastEvent.Position(lastPosition, lastDuration, state == CastRules.PlayerState.Paused))
                else -> {
                    detach()
                    onEvent(end)
                }
            }
        }
    }

    /** A button on the casting bar. */
    fun command(command: CastCommand) {
        val client = playing ?: return onEvent(CastEvent.Ended)
        when (command) {
            CastCommand.Pause -> client.pause()
            CastCommand.Resume -> client.play()
            CastCommand.Back, CastCommand.Forward -> {
                val by = if (command == CastCommand.Back) BACK_SECONDS else FORWARD_SECONDS
                val target = CastRules.skipTarget(client.approximateStreamPosition / 1000.0, lastDuration, by)
                client.seek(MediaSeekOptions.Builder().setPosition((target * 1000).toLong()).build())
            }
            CastCommand.Stop -> {
                client.stop()
                cast?.sessionManager?.endCurrentSession(true)
            }
        }
    }

    /** Where playback stood on the Chromecast, for saving progress. */
    fun position(): Pair<Double, Double> = lastPosition to lastDuration

    /** Stops listening (the screen that owned it is gone); casting goes on. */
    fun close() {
        detach()
        cast?.sessionManager?.removeSessionManagerListener(sessions, CastSession::class.java)
        router.removeCallback(scan)
    }
}
