package net.haven.app.player

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.Build
import android.os.IBinder
import net.haven.app.MainActivity
import net.haven.app.R

/**
 * Keeps a radio station playing while Haven is in the background (films and
 * TV channels wait): a notification with play/pause and stop, and a media
 * session for headphones and the lock screen. Started when Haven is left with
 * a station playing; stopped when Haven comes back or the station stops.
 */
class RadioService : Service() {
    companion object {
        private const val CHANNEL = "radio"
        private const val SHOWN = 1
        private const val TOGGLE = "net.haven.app.radio.TOGGLE"
        private const val STOP = "net.haven.app.radio.STOP"

        /** The player the service controls (set by the app). */
        var player: PlayerModel? = null

        fun start(context: Context) {
            runCatching { context.startForegroundService(Intent(context, RadioService::class.java)) }
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, RadioService::class.java))
        }
    }

    private var session: MediaSession? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(NotificationChannel(CHANNEL, "Radio", NotificationManager.IMPORTANCE_LOW))
        session = MediaSession(this, "Haven radio").apply {
            setCallback(object : MediaSession.Callback() {
                override fun onPlay() = toggle(play = true)
                override fun onPause() = toggle(play = false)
                override fun onStop() = stopRadio()
            })
            isActive = true
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            TOGGLE -> toggle(null)
            STOP -> return stopRadio().let { START_NOT_STICKY }
        }
        show()
        return START_NOT_STICKY
    }

    private fun toggle(play: Boolean?) {
        val player = player ?: return
        if (play == null || play == player.paused) player.togglePause()
        show()
    }

    private fun stopRadio() {
        player?.close()
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    /** The notification and the session say what plays, and whether it is paused. */
    private fun show() {
        val player = player
        val paused = player?.paused == true
        session?.setPlaybackState(
            PlaybackState.Builder()
                .setActions(PlaybackState.ACTION_PLAY or PlaybackState.ACTION_PAUSE or PlaybackState.ACTION_PLAY_PAUSE or PlaybackState.ACTION_STOP)
                .setState(if (paused) PlaybackState.STATE_PAUSED else PlaybackState.STATE_PLAYING, PlaybackState.PLAYBACK_POSITION_UNKNOWN, 1f)
                .build(),
        )
        session?.setMetadata(
            android.media.MediaMetadata.Builder()
                .putString(android.media.MediaMetadata.METADATA_KEY_TITLE, player?.title ?: "Radio")
                .putString(android.media.MediaMetadata.METADATA_KEY_ARTIST, player?.subtitle ?: "")
                .build(),
        )
        fun action(action: String) = PendingIntent.getService(this, action.hashCode(), Intent(this, RadioService::class.java).setAction(action), PendingIntent.FLAG_IMMUTABLE)
        val open = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP), PendingIntent.FLAG_IMMUTABLE)
        val notification = Notification.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_radio)
            .setContentTitle(player?.title ?: "Radio")
            .setContentText(player?.subtitle?.ifEmpty { null } ?: "Haven radio")
            .setContentIntent(open)
            .setOngoing(!paused)
            .addAction(Notification.Action.Builder(null, if (paused) "Play" else "Pause", action(TOGGLE)).build())
            .addAction(Notification.Action.Builder(null, "Stop", action(STOP)).build())
            .setStyle(Notification.MediaStyle().setMediaSession(session?.sessionToken).setShowActionsInCompactView(0, 1))
            .build()
        if (Build.VERSION.SDK_INT >= 29) startForeground(SHOWN, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
        else startForeground(SHOWN, notification)
    }

    override fun onDestroy() {
        session?.release()
        session = null
        super.onDestroy()
    }
}
