package net.haven.app.player

/** When a title is remembered, resumed, or counted as watched (Rust
 *  `haven-player::resume`). */
object Resume {
    /** Nothing is remembered before this many seconds have been watched. */
    const val KEEP_AFTER = 30.0
    /** From this share of the running time on, the title counts as watched. */
    const val WATCHED_FROM = 0.95
    /** It also counts as watched with this little left (credits skipped): at
     *  most 10 minutes, and at most a tenth of the running time, so a short
     *  episode is not marked halfway through. */
    const val WATCHED_LEFT = 600.0
    const val WATCHED_LEFT_SHARE = 0.10
    /** Resuming starts this many seconds earlier, to pick the thread back up. */
    const val REWIND = 3.0

    /** How far someone got into a video. */
    enum class Watched { Started, Partway, Finished }

    /** Judges a position; null when the running time is unknown (a live
     *  stream), where there is no place to remember. */
    fun judge(position: Double, duration: Double): Watched? {
        if (!position.isFinite() || !duration.isFinite() || duration <= 0.0) return null
        val left = duration - position
        return when {
            position >= duration * WATCHED_FROM || left <= minOf(WATCHED_LEFT, duration * WATCHED_LEFT_SHARE) -> Watched.Finished
            position >= KEEP_AFTER -> Watched.Partway
            else -> Watched.Started
        }
    }

    /** Where to start a title that was left at `position`; null to start from
     *  the beginning. */
    fun resumeFrom(position: Double, duration: Double, finished: Boolean): Double? {
        if (finished || judge(position, duration) != Watched.Partway) return null
        return maxOf(position - REWIND, 0.0)
    }

    /** The share watched, from 0 to 1, for a progress bar. */
    fun share(position: Double, duration: Double): Float {
        if (!position.isFinite() || !duration.isFinite() || duration <= 0.0) return 0f
        return (position / duration).coerceIn(0.0, 1.0).toFloat()
    }

    /** "4:05", or "1:02:03" once there are hours. */
    fun clock(seconds: Double): String {
        val total = if (seconds.isFinite()) maxOf(seconds, 0.0).toLong() else 0L
        val h = total / 3600
        val m = total / 60 % 60
        val s = total % 60
        return if (h > 0) "%d:%02d:%02d".format(h, m, s) else "%d:%02d".format(m, s)
    }
}
