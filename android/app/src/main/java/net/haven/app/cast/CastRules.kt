package net.haven.app.cast

import net.haven.app.player.Resume
import java.net.URI

/** The parts of casting that need no device: what may be cast, what the
 *  Chromecast is told, and what the casting bar shows. */
object CastRules {
    /** The media type a Chromecast is told to expect, from the address. */
    fun contentType(address: String): String {
        val path = address.split('?', '#').first().lowercase()
        return when (path.substringAfterLast('.', "")) {
            "m3u8" -> "application/x-mpegURL"
            "mpd" -> "application/dash+xml"
            "webm" -> "video/webm"
            "mkv" -> "video/x-matroska"
            "mp3" -> "audio/mpeg"
            "aac", "m4a" -> "audio/mp4"
            else -> "video/mp4"
        }
    }

    /** Why an address cannot go to a Chromecast, or null when it can. The
     *  Chromecast fetches the address itself, so it must be a web address it
     *  can reach: not a file on this device, and not this device's own
     *  helpers (127.0.0.1, localhost). */
    fun whyNot(address: String): String? {
        val uri = runCatching { URI(address.trim()) }.getOrNull() ?: return "the address is not one a Chromecast can open"
        val scheme = uri.scheme?.lowercase()
        if (scheme != "http" && scheme != "https") return "only web addresses can be cast; this one plays on this device only"
        val host = uri.host?.lowercase()?.trim('[', ']') ?: return "the address is not one a Chromecast can open"
        if (host == "localhost" || host.endsWith(".localhost") || host.startsWith("127.") || host == "::1" || host == "0.0.0.0") {
            return "this stream comes through Haven on this device, so a Chromecast cannot reach it"
        }
        return null
    }

    /** Where casting starts: where the title was left, as playing here would. */
    fun startAt(position: Double?, duration: Double?, finished: Boolean): Double =
        if (position == null || duration == null) 0.0 else Resume.resumeFrom(position, duration, finished) ?: 0.0

    /** The title line on the casting bar. */
    fun titleLine(title: String, detail: String) = if (detail.isEmpty()) title else "$title · $detail"

    /** The time on the casting bar. */
    fun timeText(position: Double, duration: Double) =
        if (duration > 0) "${Resume.clock(position)} / ${Resume.clock(duration)}" else Resume.clock(position)

    /** Where a skip lands: never before the start, nor past the end. */
    fun skipTarget(position: Double, duration: Double, seconds: Double): Double {
        val target = maxOf(position + seconds, 0.0)
        return if (duration > 0) minOf(target, duration) else target
    }

    /** What the Chromecast reported, as Haven reads it. */
    enum class PlayerState { Unknown, Idle, Buffering, Playing, Paused, Loading }
    enum class IdleReason { None, Finished, Cancelled, Interrupted, Error }

    /** Whether the Chromecast has stopped for good, and how: null while it
     *  is still playing (or about to). */
    fun ending(state: PlayerState, reason: IdleReason): CastEvent? = when {
        state != PlayerState.Idle -> null
        reason == IdleReason.Error -> CastEvent.Failed("the Chromecast could not play this stream")
        reason == IdleReason.None -> null
        else -> CastEvent.Ended
    }
}

/** A Chromecast (or other Cast device) on the network. */
data class CastDevice(val id: String, val name: String)

/** News from a casting session (as the Rust build's `haven_cast::Event`). */
sealed class CastEvent {
    /** Connected; the stream is being handed over. */
    data class Connecting(val device: String) : CastEvent()
    /** The Chromecast accepted the address. */
    data object Started : CastEvent()
    data class Position(val position: Double, val duration: Double, val paused: Boolean) : CastEvent()
    /** Playback is over (finished, stopped here, or stopped on the device). */
    data object Ended : CastEvent()
    data class Failed(val reason: String) : CastEvent()
}

/** What to play on the Chromecast. */
data class CastRequest(
    val address: String,
    val title: String,
    val detail: String = "",
    /** Seconds in to start at. */
    val start: Double = 0.0,
    val poster: String? = null,
)

enum class CastCommand { Pause, Resume, Back, Forward, Stop }
