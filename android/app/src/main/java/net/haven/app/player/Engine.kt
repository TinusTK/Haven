package net.haven.app.player

/** What plays the picture and sound: libmpv on a phone or TV ([MpvEngine]),
 *  a stand-in in the tests. Every call is made on the main thread. */
interface PlayerEngine {
    /** What mpv reported since the last [poll]. */
    sealed class Event {
        /** The file is open and about to play. */
        data object Loaded : Event()
        /** Played to the end. */
        data object Finished : Event()
        /** Could not be opened, or broke off; mpv's reason in words. */
        data class Failed(val reason: String) : Event()
    }

    /** Where playback stands right now. */
    data class Status(
        val position: Double,
        /** 0 while unknown and for live streams. */
        val duration: Double,
        val paused: Boolean,
        /** Waiting for more of the stream. */
        val buffering: Boolean,
        /** Bytes a second arriving, if known. */
        val arriving: Double? = null,
        /** Seconds past the position that have already arrived. */
        val ahead: Double? = null,
    )

    /** How the picture fills the screen. */
    enum class Fit { Whole, Fill, Stretch }

    /** What mpv knows about what is playing. */
    data class Details(
        val width: Int? = null,
        val height: Int? = null,
        val framesPerSecond: Double? = null,
        val video: String? = null,
        val audio: String? = null,
        val channels: String? = null,
        val container: String? = null,
        /** The phone's own decoder, when it is the one in use. */
        val decoder: String? = null,
    )

    /** Why the player cannot work here, once known. */
    val broken: String?
    /** Null until mpv is running. */
    val status: Status?
    /** mpv's track list as it gave it (JSON); empty when nothing is loaded. */
    val tracksJson: String
    val details: Details

    /** Plays a web address (or a file here), from `at` seconds in or from the
     *  start. Whatever was playing is replaced. */
    fun play(address: String, at: Double?)
    /** Stops playing and empties the read-ahead; mpv stays ready. */
    fun stop()
    fun poll(): List<Event>
    fun setPaused(paused: Boolean)
    fun seekTo(seconds: Double)
    fun seekBy(seconds: Double)
    fun selectAudio(id: Int)
    /** Null turns subtitles off. */
    fun selectSubtitle(id: Int?)
    /** How far subtitles sit above the bottom (the picture is 720 high). */
    fun setSubtitleMargin(margin: Int)
    /** 0 silent to 1 as recorded, up to 2 (200%, amplified by mpv). */
    fun setVolume(volume: Double)
    fun setMuted(muted: Boolean)
    /** Dolby and DTS sound sent on as it is, for a receiver to decode. */
    fun setPassthrough(on: Boolean)
    /** 1 is normal speed; the pitch is kept. */
    fun setSpeed(speed: Double)
    fun setFit(fit: Fit)
    /** 1 is the usual size. */
    fun setSubtitleScale(scale: Double)
    /** Shows subtitles this many seconds later (earlier if negative). */
    fun setSubtitleDelay(seconds: Double)
    /** Moves the subtitles so the next line (1) or the one before (-1) shows
     *  now. Returns the delay that results. */
    fun syncSubtitles(way: Int): Double?

    companion object {
        /** mpv's reason for giving up on a file, in words. */
        fun reason(code: Int): String = when (code) {
            -13 -> "the address could not be opened"
            -17 -> "the format was not recognised"
            -16, -18 -> "there was no picture or sound in it"
            else -> "mpv error $code"
        }
    }
}
