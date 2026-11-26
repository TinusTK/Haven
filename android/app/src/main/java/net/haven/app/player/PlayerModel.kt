package net.haven.app.player

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableDoubleStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import net.haven.app.core.Log
import net.haven.app.core.Store
import net.haven.app.core.WatchProgress
import net.haven.app.library.PlayRequest

/** How far a film or episode got, for the books (sync, the media server, the
 *  next episode). */
data class Played(
    val kind: String,
    val titleId: String,
    val videoId: String,
    val position: Double,
    val duration: Double,
    val finished: Boolean,
    /** The media server item it played, if it came from one. */
    val serverItem: String? = null,
)

/**
 * The player page: the video, its controls, and remembering how far each film
 * or episode was watched. The Rust build's `haven-app/src/player.rs`.
 */
class PlayerModel(
    val store: Store,
    val engine: PlayerEngine,
    private val scope: CoroutineScope,
    /** A TV: no volume slider, the Sound boost instead. */
    val tv: Boolean = false,
) {
    /** Which list is open. `Sound` is a TV's Sound boost. */
    enum class Menu { None, Audio, Subtitles, Speed, Picture, Info, Sound }

    private data class Playing(
        val request: PlayRequest,
        /** mpv has opened the stream. */
        var loaded: Boolean = false,
        var failed: Boolean = false,
        var position: Double,
        var duration: Double = 0.0,
        var savedAt: Long = System.currentTimeMillis(),
    )

    // What the page shows.
    var open by mutableStateOf(false)
        private set
    var title by mutableStateOf("")
        private set
    var subtitle by mutableStateOf("")
        private set
    var paused by mutableStateOf(false)
        private set
    /** Opening the stream, or waiting for more of it. */
    var busy by mutableStateOf(false)
        private set
    var error by mutableStateOf("")
        private set
    var duration by mutableDoubleStateOf(0.0)
        private set
    var position by mutableDoubleStateOf(0.0)
        private set
    /** How far the stream has arrived, in seconds from the start. */
    var buffered by mutableDoubleStateOf(0.0)
        private set
    var elapsedText by mutableStateOf("")
        private set
    var remainingText by mutableStateOf("")
        private set
    var endsAt by mutableStateOf("")
        private set
    /** "4.2 MB/s  ·  42% loaded" */
    var net by mutableStateOf("")
        private set
    var audioTracks by mutableStateOf(listOf<Track>())
        private set
    var subtitleTracks by mutableStateOf(listOf<Track>())
        private set
    /** A subtitle track is showing. */
    var anySubtitle by mutableStateOf(false)
        private set
    var menu by mutableStateOf(Menu.None)
    var preferences by mutableStateOf(PlayerRules.Preferences.load(store))
        private set
    var speed by mutableDoubleStateOf(1.0)
        private set
    var fit by mutableStateOf(PlayerEngine.Fit.Whole)
        private set
    var subtitleDelay by mutableDoubleStateOf(0.0)
        private set
    var info by mutableStateOf("")
        private set
    /** The episode after this one, in words; empty if there is none. */
    var nextTitle by mutableStateOf("")
        private set
    /** Seconds until the next episode starts by itself; null while not counting. */
    var nextCountdown by mutableStateOf<Int?>(null)
        private set
    /** The controls are on screen. */
    var controlsShown by mutableStateOf(true)
    /** The screen's height in dp, to keep subtitles clear of the controls. */
    var screenHeight = 720.0

    /** Runs each time the page closes, with how far it got. */
    var closed: (Played?) -> Unit = {}
    /** The episode after the one playing is wanted; says whether it is coming. */
    var next: ((Played?) -> Boolean)? = null

    private var playing: Playing? = null
    private var seekHeld: Long? = null
    private var nextAt: Long? = null
    private var tracksShown = ""
    private var marginSet: Int? = null
    private var ticker: Job? = null

    /** A radio station is playing: it goes on in the background. */
    val isRadio get() = playing?.request?.kind == "radio"

    // Opening and closing

    /** Opens the player page and plays the stream, from where it was left if
     *  it was left partway. */
    fun play(request: PlayRequest) {
        val at = if (!request.remember) null else runCatching { store.progress(request.kind, request.titleId, request.videoId) }.getOrNull()?.let {
            Resume.resumeFrom(it.position, it.duration, it.finished)
        }
        title = request.title
        subtitle = request.detail
        error = ""
        busy = true
        paused = false
        position = at ?: 0.0
        duration = 0.0
        buffered = 0.0
        elapsedText = ""
        remainingText = ""
        endsAt = ""
        net = ""
        info = ""
        nextTitle = request.next ?: ""
        nextCountdown = null
        nextAt = null
        // Each film or episode has its own subtitle timing.
        subtitleDelay = 0.0
        anySubtitle = false
        menu = Menu.None
        audioTracks = emptyList()
        subtitleTracks = emptyList()
        tracksShown = ""
        seekHeld = null
        marginSet = null
        controlsShown = true
        open = true
        playing = Playing(request, position = at ?: 0.0)
        startTicking()
        engine.broken?.let {
            fail("Haven's player cannot start on this device. ($it)")
            return
        }
        engine.play(request.address, at)
    }

    private fun startTicking() {
        if (ticker?.isActive == true) return
        ticker = scope.launch {
            while (isActive) {
                delay(250)
                tick()
            }
        }
    }

    private fun stopTicking() {
        ticker?.cancel()
        ticker = null
    }

    private fun fail(message: String) {
        val current = playing ?: return
        current.failed = true
        engine.stop()
        busy = false
        error = message
    }

    /** Saves the place, stops and goes back to the page underneath. */
    fun close() {
        if (playing == null) {
            dismiss()
            return
        }
        save()
        nextAt = null
        nextCountdown = null
        val played = takePlayed()
        engine.stop()
        open = false
        stopTicking()
        closed(played)
    }

    /** Leaves the page when nothing is playing (the next episode could not be
     *  started by itself). */
    fun dismiss() {
        if (playing != null) return
        nextAt = null
        nextCountdown = null
        open = false
        stopTicking()
    }

    /** Haven went to the background: the place is kept and the film waits
     *  (radio goes on). */
    fun leaving() {
        if (playing == null || isRadio) return
        save()
        engine.setPaused(true)
        paused = true
    }

    // Ticking

    /** Four times a second while the page is open: mpv's news, the position,
     *  the track menus, and saving the place now and then. */
    fun tick() {
        // The episode is over and the next one is counting down.
        nextAt?.let { at ->
            val left = at - System.currentTimeMillis()
            nextCountdown = maxOf(((left + 999) / 1000).toInt(), 0)
            if (left <= 0) goNext()
            return
        }
        val current = playing ?: return
        if (current.failed) return
        for (event in engine.poll()) {
            when (event) {
                PlayerEngine.Event.Loaded -> {
                    current.loaded = true
                    applySettings()
                }
                PlayerEngine.Event.Finished -> return finished()
                is PlayerEngine.Event.Failed -> {
                    // What was watched before it broke off still counts.
                    save()
                    return fail("This stream could not be played. (${event.reason})")
                }
            }
        }
        val status = engine.status ?: return
        if (current.loaded) {
            current.position = status.position
            current.duration = status.duration
        }
        busy = !current.loaded || status.buffering
        if (!current.loaded) return
        paused = status.paused
        duration = maxOf(status.duration, 0.0)
        val left = maxOf(status.duration - status.position, 0.0)
        net = PlayerRules.arrival(status.arriving, PlayerRules.shareLoaded(status))
        buffered = status.position + (status.ahead ?: 0.0)
        elapsedText = Resume.clock(status.position)
        remainingText = if (status.duration > 0) "−${Resume.clock(left)}" else "Live"
        endsAt = if (status.duration > 0) PlayerRules.endsAt(left, speed) else ""
        if (menu == Menu.Info) info = PlayerRules.info(engine.details, current.request.source)
        if (seekHeld?.let { System.currentTimeMillis() - it >= PlayerRules.SEEK_HOLD_MILLIS } != false) position = status.position
        showTracks()
        placeSubtitles()
        if (System.currentTimeMillis() - current.savedAt >= PlayerRules.SAVE_EVERY_MILLIS && !status.paused) save()
    }

    private fun showTracks() {
        val json = engine.tracksJson
        if (json == tracksShown) return
        tracksShown = json
        val tracks = PlayerRules.tracks(json)
        audioTracks = tracks.filter { it.kind == Track.Kind.Audio }
        subtitleTracks = tracks.filter { it.kind == Track.Kind.Subtitle }
        anySubtitle = subtitleTracks.any { it.selected }
    }

    /** Keeps subtitles clear of the controls while those are on screen. */
    fun placeSubtitles() {
        val margin = if (controlsShown) Math.round(PlayerRules.CONTROLS_HEIGHT * 720 / maxOf(screenHeight, 1.0)).toInt() else PlayerRules.SUBTITLE_MARGIN
        if (marginSet != margin) {
            engine.setSubtitleMargin(margin)
            marginSet = margin
        }
    }

    // Remembering

    /** Saves how far the film or episode got, if worth remembering. */
    fun save() {
        val current = playing ?: return
        current.savedAt = System.currentTimeMillis()
        remember(current.request, current.position, current.duration, store)
    }

    /** How far what was playing got; takes it off the player. */
    private fun takePlayed(): Played? {
        val current = playing ?: return null
        playing = null
        return played(current.request, current.position, current.duration, current.failed)
    }

    /** Played to the end: an episode with another after it counts down to that
     *  one; anything else closes the page. */
    private fun finished() {
        val current = playing ?: return
        if (current.duration > 0) current.position = current.duration
        if (current.request.next == null || next == null) return close()
        save()
        nextAt = System.currentTimeMillis() + PlayerRules.NEXT_AFTER_MILLIS
        busy = false
        paused = true
        nextCountdown = (PlayerRules.NEXT_AFTER_MILLIS / 1000).toInt()
    }

    /** Moves on to the next episode without leaving the player; if there turns
     *  out to be none, the page closes. */
    fun goNext() {
        if (playing == null) return
        nextAt = null
        save()
        val played = takePlayed()
        engine.stop()
        nextCountdown = null
        menu = Menu.None
        subtitle = nextTitle
        nextTitle = ""
        error = ""
        busy = true
        duration = 0.0
        position = 0.0
        elapsedText = ""
        remainingText = ""
        endsAt = ""
        val coming = next?.invoke(played) ?: false
        if (!coming) dismiss()
    }

    /** The next episode's countdown was stopped: the page closes. */
    fun cancelNext() = close()

    // Controls

    fun togglePause() {
        val status = engine.status ?: return
        engine.setPaused(!status.paused)
        paused = !status.paused
        // Pausing is a good moment to remember the place.
        if (!status.paused) save()
    }

    fun seekTo(seconds: Double) {
        engine.seekTo(seconds)
        hold(seconds)
    }

    fun skip(seconds: Double) {
        val status = engine.status ?: return
        engine.seekBy(seconds)
        var target = maxOf(status.position + seconds, 0.0)
        if (status.duration > 0) target = minOf(target, status.duration)
        hold(target)
    }

    private fun hold(seconds: Double) {
        seekHeld = System.currentTimeMillis()
        position = seconds
    }

    fun chooseAudio(id: Int) {
        engine.selectAudio(id)
        menu = Menu.None
    }

    /** Null turns subtitles off. */
    fun chooseSubtitle(id: Int?) {
        engine.selectSubtitle(id)
        menu = Menu.None
    }

    /** Moving the volume turns the sound back on. */
    fun setVolume(volume: Double) = change { it.copy(volume = volume.coerceIn(0.0, it.top(tv)), muted = false) }

    /** Turned off, a volume above 100% comes back down to 100%. */
    fun setLoud(on: Boolean) = change { p -> p.copy(loud = on).let { it.copy(volume = minOf(it.volume, it.top(tv))) } }

    fun toggleMute() = change { it.copy(muted = !it.muted) }

    fun setPassthrough(on: Boolean) = change { it.copy(passthrough = on) }

    fun stepSubtitleScale(way: Int) = change {
        it.copy(subtitleScale = Math.rint(((it.subtitleScale + 0.1 * Integer.signum(way)) * 10).coerceIn(5.0, 25.0)) / 10)
    }

    /** Half a second later or earlier. */
    fun stepSubtitleDelay(way: Int) {
        subtitleDelay = (subtitleDelay + 0.5 * Integer.signum(way)).coerceIn(-60.0, 60.0)
        applySettings()
    }

    /** 1: the next line shows now; -1: the line before; 0: no delay. */
    fun syncSubtitles(way: Int) {
        val delay = if (way == 0) 0.0 else engine.syncSubtitles(way)
        if (delay != null) {
            subtitleDelay = delay.coerceIn(-60.0, 60.0)
            applySettings()
        }
    }

    fun chooseSpeed(speed: Double) {
        this.speed = speed.coerceIn(0.25, 4.0)
        applySettings()
        menu = Menu.None
    }

    fun chooseFit(fit: PlayerEngine.Fit) {
        this.fit = fit
        applySettings()
        menu = Menu.None
    }

    fun toggleMenu(wanted: Menu) {
        menu = if (menu == wanted) Menu.None else wanted
        val current = playing
        if (menu == Menu.Info && current != null) info = PlayerRules.info(engine.details, current.request.source)
    }

    private fun change(edit: (PlayerRules.Preferences) -> PlayerRules.Preferences) {
        preferences = edit(preferences)
        runCatching { store.put(PlayerRules.Preferences.KEY, preferences) }.onFailure { Log.note("could not save the player's settings: ${it.message}") }
        applySettings()
    }

    /** Gives mpv the volume, speed and the like. */
    private fun applySettings() {
        engine.setVolume(preferences.volume)
        engine.setMuted(preferences.muted)
        engine.setSubtitleScale(preferences.subtitleScale)
        engine.setPassthrough(preferences.passthrough)
        engine.setSpeed(speed)
        engine.setFit(fit)
        engine.setSubtitleDelay(subtitleDelay)
    }

    companion object {
        /** Keeps the place of a film or episode, if it is worth remembering. */
        fun remember(request: PlayRequest, position: Double, duration: Double, store: Store) {
            if (!request.remember) return
            val finished = when (Resume.judge(position, duration)) {
                Resume.Watched.Partway -> false
                Resume.Watched.Finished -> true
                else -> return
            }
            runCatching {
                store.saveProgress(
                    WatchProgress(request.kind, request.titleId, request.videoId, request.title, request.detail, request.poster,
                        position, duration, finished, System.currentTimeMillis()),
                )
            }.onFailure { Log.note("could not save where this was left: ${it.message}") }
        }

        /** How far it got, for the books; null when barely started or not kept. */
        fun played(request: PlayRequest, position: Double, duration: Double, failed: Boolean): Played? {
            if (!request.remember || failed) return null
            val judged = Resume.judge(position, duration) ?: return null
            if (judged == Resume.Watched.Started) return null
            return Played(request.kind, request.titleId, request.videoId, position, duration, judged == Resume.Watched.Finished, request.serverItem)
        }
    }
}
