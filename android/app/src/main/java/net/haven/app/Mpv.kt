package net.haven.app

/**
 * Haven's own bridge to libmpv (src/main/cpp/mpv_bridge.c). The player is
 * written in section K13; for now it only proves libmpv loads.
 */
object Mpv {
    private val loaded: Result<Unit> = runCatching {
        System.loadLibrary("mpv")
        System.loadLibrary("havenmpv")
    }

    /** libmpv's client API version (major << 16 | minor), or null when it
     *  could not be loaded (unit tests on a computer, say). */
    fun apiVersion(): Long? = loaded.getOrNull()?.let { clientApiVersion() }

    private external fun clientApiVersion(): Long
}
