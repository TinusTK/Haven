package net.haven.app

import net.haven.app.player.MpvNative

/** libmpv's version, for the log and the start screen. */
object Mpv {
    /** libmpv's client API version (major << 16 | minor), or null when it
     *  could not be loaded (unit tests on a computer, say). */
    fun apiVersion(): Long? = MpvNative.loaded.getOrNull()?.let { runCatching { MpvNative.clientApiVersion() }.getOrNull() }
}
