package net.haven.app.player

/** The calls into libmpv (src/main/cpp/mpv_bridge.c). `h` is mpv's handle. */
internal object MpvNative {
    val loaded: Result<Unit> = runCatching {
        System.loadLibrary("mpv")
        System.loadLibrary("havenmpv")
    }

    external fun clientApiVersion(): Long
    external fun create(): Long
    external fun initialize(h: Long): Int
    external fun destroy(h: Long)
    external fun setOption(h: Long, name: String, value: String): Int
    external fun setProperty(h: Long, name: String, value: String): Int
    external fun getString(h: Long, name: String): String?
    external fun getDouble(h: Long, name: String, out: DoubleArray): Int
    external fun getLong(h: Long, name: String, out: LongArray): Int
    /** 1 yes, 0 no, -1 not known. */
    external fun getFlag(h: Long, name: String): Int
    external fun command(h: Long, words: Array<String>): Int
    /** [event id, end-file reason, end-file error]; id 0 when there is none. */
    external fun nextEvent(h: Long): IntArray
    /** Starts drawing on `surface`; false if OpenGL ES or mpv's renderer would not start. */
    external fun attachSurface(h: Long, surface: android.view.Surface): Boolean
    external fun detachSurface(h: Long)
    external fun redraw()
}
