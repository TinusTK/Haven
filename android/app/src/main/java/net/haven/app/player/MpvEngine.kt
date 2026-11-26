package net.haven.app.player

import android.content.Context
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.view.Surface
import net.haven.app.core.Log
import java.io.File
import java.util.Locale

/**
 * mpv, Haven's player, drawing straight onto a Surface (vo=gpu with Android's
 * own OpenGL ES, as mpv-android does). mpv is created off the main thread the
 * first time something plays and lives until the app ends; each film replaces
 * the last. Like the desktop build's `haven-player/src/video.rs`.
 */
class MpvEngine(context: Context) : PlayerEngine {
    private val resolver = context.applicationContext.contentResolver
    private val main = Handler(Looper.getMainLooper())
    /** mpv puts what it has read ahead here, on disk; emptied when a film closes. */
    private val cache = File(context.cacheDir, "haven-stream")
    private val fonts = File(context.cacheDir, "haven-fonts")

    private var handle = 0L
    private var starting = false
    override var broken: String? = MpvNative.loaded.exceptionOrNull()?.let { "libmpv is missing (${it.message})" }
        private set
    /** What to load as soon as mpv can draw. */
    private var waiting: Pair<String, Double?>? = null
    private var surface: Surface? = null
    /** mpv is drawing on the Surface. */
    private var drawing = false

    private fun start() {
        if (handle != 0L || starting || broken != null) return
        starting = true
        Thread({
            val made = runCatching { create() }
            main.post {
                starting = false
                made.fold(
                    onSuccess = {
                        handle = it
                        surface?.let(::attach)
                        flush()
                    },
                    onFailure = {
                        broken = it.message
                        Log.note("the player cannot work here: ${it.message}")
                    },
                )
            }
        }, "haven-mpv").start()
    }

    private fun create(): Long {
        val mpv = MpvNative.create()
        if (mpv == 0L) error("mpv could not start")
        cache.mkdirs()
        linkFonts()
        fun option(name: String, value: String) = MpvNative.setOption(mpv, name, value)
        // mpv draws through its render API into Haven's own OpenGL ES context.
        option("vo", "libmpv")
        // A paused picture redrawn through mpv's default half-float
        // framebuffer came out black on the emulator; 8 bits a colour works.
        option("fbo-format", "rgba8")
        // The phone's own decoders (MediaCodec), copied back for drawing. The
        // emulator's decoder gives mpv no pictures, so it decodes in software.
        option("hwdec", if (emulator()) "no" else "mediacodec-copy")
        option("ao", "audiotrack,opensles")
        // Android has no font list for subtitles: a few of the phone's own.
        option("sub-fonts-dir", fonts.path)
        option("osd-fonts-dir", fonts.path)
        option("sub-font", "Roboto")
        option("osd-font", "Roboto")
        option("sub-scale-by-window", "no")
        option("video-timing-offset", "0")
        // The volume may go up to 200% (mpv stops at 130% unless told).
        option("volume-max", "200")
        // Addresses are opened as they are: no helper programs.
        option("ytdl", "no")
        option("sub-auto", "no")
        option("audio-display", "no")
        option("network-timeout", "30")
        // Fetch the whole film, not just the next minutes: the read-ahead goes
        // to a file on disk, and what was watched is kept for going back.
        option("cache", "yes")
        option("cache-on-disk", "yes")
        option("demuxer-cache-dir", cache.path)
        option("demuxer-max-bytes", "200GiB")
        option("demuxer-max-back-bytes", "200GiB")
        option("cache-secs", "1000000")
        option("demuxer-readahead-secs", "1000000")
        // Haven keeps no settings of mpv's own and loads no scripts.
        option("config", "no")
        option("load-scripts", "no")
        option("input-default-bindings", "no")
        option("keep-open", "no")
        // Debug builds keep mpv's own log beside the stream cache.
        if (net.haven.app.BuildConfig.DEBUG) {
            option("log-file", File(cache.parentFile, "mpv.log").path)
            option("msg-level", "all=info")
        }
        val done = MpvNative.initialize(mpv)
        if (done < 0) {
            MpvNative.destroy(mpv)
            error("mpv could not start (error $done)")
        }
        return mpv
    }

    /** A folder of links to the phone's fonts subtitles need most (Latin,
     *  Chinese/Japanese/Korean, Arabic, Hebrew, Thai). */
    private fun linkFonts() {
        fonts.mkdirs()
        for (name in listOf("Roboto-Regular.ttf", "NotoSansCJK-Regular.ttc", "NotoNaskhArabic-Regular.ttf", "NotoSansHebrew-Regular.ttf", "NotoSansThai-Regular.ttf")) {
            val from = File("/system/fonts", name)
            val to = File(fonts, name)
            if (from.exists() && !to.exists()) runCatching { android.system.Os.symlink(from.path, to.path) }
        }
    }

    private fun emulator() = Build.HARDWARE.contains("ranchu") || Build.HARDWARE.contains("goldfish") || Build.FINGERPRINT.contains("generic")

    // The Surface

    /** The Surface to draw on came (the player is on screen). */
    fun surfaceCreated(view: Surface) {
        surface = view
        if (handle != 0L) {
            attach(view)
            flush()
        } else {
            start()
        }
    }

    private fun attach(view: Surface) {
        drawing = MpvNative.attachSurface(handle, view)
        if (!drawing) {
            broken = "the picture cannot be drawn on this device"
            Log.note("the player cannot draw here")
            return
        }
        // A film that was playing gets its picture back.
        MpvNative.setProperty(handle, "vid", "auto")
    }

    fun surfaceChanged(width: Int, height: Int) {
        if (drawing) MpvNative.redraw()
    }

    /** The Surface went (the player left the screen, or Haven went away). */
    fun surfaceDestroyed() {
        surface = null
        if (handle == 0L || !drawing) return
        drawing = false
        MpvNative.detachSurface(handle)
    }

    private fun flush() {
        val (address, at) = waiting ?: return
        if (handle == 0L || !drawing) return
        waiting = null
        load(address, at)
    }

    // Playing

    override fun play(address: String, at: Double?) {
        if (broken != null) return
        waiting = address to at
        if (handle == 0L) start() else flush()
    }

    private fun load(address: String, at: Double?) {
        val opened = playable(address)
        set("start", at?.let { String.format(Locale.ROOT, "%.1f", it) } ?: "none")
        set("pause", "no")
        command("loadfile", opened, "replace")
    }

    /** A video from the device's media list (`content://`) is opened here and
     *  handed over as an open file (`fdclose://<number>`, which mpv closes
     *  when it is done); anything else plays as it is. */
    private fun playable(address: String): String {
        if (!address.startsWith("content://")) return address
        return runCatching { "fdclose://" + resolver.openFileDescriptor(android.net.Uri.parse(address), "r")!!.detachFd() }
            .getOrElse {
                Log.note("player: could not open a video on this device: ${it.message}")
                address
            }
    }

    override fun stop() {
        waiting = null
        if (handle != 0L) command("stop")
        Thread { cache.listFiles()?.forEach { it.deleteRecursively() } }.start()
    }

    override fun poll(): List<PlayerEngine.Event> {
        if (handle == 0L) return emptyList()
        val events = mutableListOf<PlayerEngine.Event>()
        while (true) {
            val (id, reason, error) = MpvNative.nextEvent(handle).let { Triple(it[0], it[1], it[2]) }
            when (id) {
                0 -> break
                8 -> events += PlayerEngine.Event.Loaded
                7 -> when (reason) {
                    0 -> events += PlayerEngine.Event.Finished
                    4 -> events += PlayerEngine.Event.Failed(PlayerEngine.reason(error))
                }
            }
        }
        return events
    }

    override val status: PlayerEngine.Status?
        get() = if (handle == 0L) null else PlayerEngine.Status(
            double("time-pos") ?: 0.0, double("duration") ?: 0.0, flag("pause") ?: false, flag("paused-for-cache") ?: false,
            whole("cache-speed")?.let { maxOf(it, 0L).toDouble() }, double("demuxer-cache-duration")?.let { maxOf(it, 0.0) },
        )

    override fun setPaused(paused: Boolean) = set("pause", if (paused) "yes" else "no")
    override fun seekTo(seconds: Double) = command("seek", String.format(Locale.ROOT, "%.2f", maxOf(seconds, 0.0)), "absolute")
    override fun seekBy(seconds: Double) = command("seek", String.format(Locale.ROOT, "%.2f", seconds), "relative")
    override val tracksJson: String get() = text("track-list") ?: ""
    override fun selectAudio(id: Int) = set("aid", id.toString())
    override fun selectSubtitle(id: Int?) = set("sid", id?.toString() ?: "no")
    override fun setSubtitleMargin(margin: Int) = set("sub-margin-y", margin.coerceIn(0, 400).toString())
    override fun setVolume(volume: Double) = set("volume", Math.round(volume.coerceIn(0.0, 2.0) * 100).toString())
    override fun setMuted(muted: Boolean) = set("mute", if (muted) "yes" else "no")
    override fun setPassthrough(on: Boolean) = set("audio-spdif", if (on) "ac3,eac3,dts,dts-hd,truehd" else "")
    override fun setSpeed(speed: Double) = set("speed", speed.coerceIn(0.25, 4.0).toString())
    override fun setFit(fit: PlayerEngine.Fit) {
        set("keepaspect", if (fit == PlayerEngine.Fit.Stretch) "no" else "yes")
        set("panscan", if (fit == PlayerEngine.Fit.Fill) "1.0" else "0.0")
    }
    override fun setSubtitleScale(scale: Double) = set("sub-scale", scale.coerceIn(0.3, 3.0).toString())
    override fun setSubtitleDelay(seconds: Double) = set("sub-delay", seconds.coerceIn(-60.0, 60.0).toString())
    override fun syncSubtitles(way: Int): Double? {
        if (handle == 0L) return null
        command("sub-step", if (way < 0) "-1" else "1")
        return double("sub-delay")
    }

    override val details: PlayerEngine.Details
        get() {
            fun trimmed(name: String) = text(name)?.trim()?.takeIf { it.isNotEmpty() }
            return PlayerEngine.Details(
                whole("width")?.takeIf { it > 0 }?.toInt(), whole("height")?.takeIf { it > 0 }?.toInt(),
                double("container-fps")?.takeIf { it > 0 }, trimmed("video-format") ?: trimmed("video-codec"),
                trimmed("audio-codec-name") ?: trimmed("audio-codec"), trimmed("audio-params/hr-channels") ?: trimmed("audio-params/channels"),
                trimmed("file-format"), trimmed("hwdec-current")?.takeIf { it != "no" },
            )
        }

    // mpv's own calls

    private fun set(name: String, value: String) {
        if (handle != 0L) MpvNative.setProperty(handle, name, value)
    }

    private fun command(vararg words: String) {
        if (handle != 0L) MpvNative.command(handle, arrayOf(*words))
    }

    private fun text(name: String): String? = if (handle == 0L) null else MpvNative.getString(handle, name)

    private fun double(name: String): Double? {
        if (handle == 0L) return null
        val out = DoubleArray(1)
        return if (MpvNative.getDouble(handle, name, out) >= 0) out[0] else null
    }

    private fun whole(name: String): Long? {
        if (handle == 0L) return null
        val out = LongArray(1)
        return if (MpvNative.getLong(handle, name, out) >= 0) out[0] else null
    }

    private fun flag(name: String): Boolean? = if (handle == 0L) null else when (MpvNative.getFlag(handle, name)) {
        1 -> true
        0 -> false
        else -> null
    }
}
