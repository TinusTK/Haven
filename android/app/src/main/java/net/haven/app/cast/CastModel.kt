package net.haven.app.cast

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import net.haven.app.core.Store
import net.haven.app.library.PlayRequest
import net.haven.app.player.PlayerModel
import net.haven.app.player.Played
import net.haven.app.player.Resume

/**
 * Casting a stream to a Chromecast instead of playing it here: the devices
 * found on the network (listed under the places to watch), the one chosen,
 * and the casting bar while one plays. The Rust build's `library/cast.rs`.
 */
class CastModel(private val store: Store, makeLink: ((CastEvent) -> Unit) -> Caster?) {
    private val link: Caster? = makeLink { event(it) }

    /** Whether this device can cast at all (Google Play services). */
    val available get() = link?.available == true

    var devices by mutableStateOf(listOf<CastDevice>())
        private set
    var finding by mutableStateOf(false)
        private set
    var note by mutableStateOf("")
        private set
    /** The Chromecast chosen; -1 plays on this device. */
    var target by mutableIntStateOf(-1)

    // The casting bar.
    var casting by mutableStateOf(false)
        private set
    var title by mutableStateOf("")
        private set
    var deviceName by mutableStateOf("")
        private set
    var status by mutableStateOf("")
        private set
    var time by mutableStateOf("")
        private set
    var paused by mutableStateOf(false)
        private set

    /** Told when a cast film or episode ends, with how far it got. */
    var closed: (Played?) -> Unit = {}

    private var request: PlayRequest? = null
    private var position = 0.0
    private var duration = 0.0
    private var savedAt = 0L

    /** The Chromecast chosen, if one is. */
    val chosen: CastDevice? get() = devices.getOrNull(target)

    /** Listens for Chromecasts and lists them under the places to watch. */
    fun find() {
        val link = link ?: run {
            note = "Casting needs Google Play services, which this device does not have."
            return
        }
        finding = true
        note = "Looking for Chromecasts on your network…"
        link.find { found ->
            finding = false
            note = if (found.isEmpty()) "No Chromecast answered on this network." else ""
            val was = chosen?.id
            devices = found
            // The chosen device may be gone, or have moved in the list.
            target = found.indexOfFirst { it.id == was }
        }
    }

    /** Why `request` cannot go to the Chromecast; null when it can. */
    fun whyNot(request: PlayRequest) = CastRules.whyNot(request.address)

    /** Plays a stream on the chosen Chromecast, from where it was left. */
    fun cast(request: PlayRequest) {
        val device = chosen ?: return
        val link = link ?: return
        val saved = if (request.remember) runCatching { store.progress(request.kind, request.titleId, request.videoId) }.getOrNull() else null
        val start = CastRules.startAt(saved?.position, saved?.duration, saved?.finished ?: false)
        finish()
        this.request = request
        position = start
        duration = 0.0
        savedAt = System.currentTimeMillis()
        casting = true
        paused = false
        time = ""
        title = CastRules.titleLine(request.title, request.detail)
        deviceName = device.name
        status = "Connecting to ${device.name}…"
        link.play(device, CastRequest(request.address, request.title, request.detail, start, request.poster))
    }

    private fun event(event: CastEvent) {
        when (event) {
            is CastEvent.Connecting -> status = "Connecting to ${event.device}…"
            CastEvent.Started -> status = ""
            is CastEvent.Position -> {
                paused = event.paused
                position = event.position
                duration = event.duration
                time = CastRules.timeText(event.position, event.duration)
                // The place is kept as Haven's player keeps it.
                if (System.currentTimeMillis() - savedAt >= 5_000) {
                    savedAt = System.currentTimeMillis()
                    request?.let { PlayerModel.remember(it, position, duration, store) }
                }
            }
            CastEvent.Ended -> {
                finish()
                casting = false
            }
            is CastEvent.Failed -> {
                finish()
                status = "Casting stopped: ${event.reason}."
                time = ""
            }
        }
    }

    /** What was casting is over: its place is kept and the books told. */
    private fun finish() {
        val done = request ?: return
        request = null
        PlayerModel.remember(done, position, duration, store)
        closed(PlayerModel.played(done, position, duration, failed = false))
    }

    /** A button on the casting bar. Nothing casting any more: the bar closes. */
    fun command(command: CastCommand) {
        if (request == null) {
            casting = false
            return
        }
        link?.command(command)
    }

    fun togglePause() = command(if (paused) CastCommand.Resume else CastCommand.Pause)

    /** "4:05 / 1:00:00" while playing. */
    val clock get() = if (duration > 0) "${Resume.clock(position)} / ${Resume.clock(duration)}" else time
}
