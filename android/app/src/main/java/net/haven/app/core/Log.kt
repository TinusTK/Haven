package net.haven.app.core

import java.io.File
import java.io.FileOutputStream

/** A small log file beside the database (`haven.log`), for finding out
 *  afterwards why something did not load. Notes must never hold passwords,
 *  tokens or keys: log an address through [plainAddress]. */
object Log {
    /** The log starts afresh once it has grown past this. */
    const val MAX_BYTES = 512 * 1024

    private var out: FileOutputStream? = null

    /** Opens the log file. Until this is called, notes only go to logcat. */
    @Synchronized
    fun start(file: File, version: String = Haven.version) {
        val fresh = !file.exists() || file.length() > MAX_BYTES
        out?.close()
        out = runCatching { FileOutputStream(file, !fresh) }.getOrNull()
        note("Haven $version started")
    }

    /** Writes one line to logcat and to the log file. */
    @Synchronized
    fun note(text: String) {
        runCatching { android.util.Log.i("haven", text) }
        val seconds = System.currentTimeMillis() / 1000
        runCatching { out?.run { write("$seconds $text\n".toByteArray()); flush() } }
    }

    /** An address without what follows `?` or `#` (where tokens and keys
     *  travel) and without `user:password@`. */
    fun plainAddress(address: String): String {
        val cut = address.split('?', '#').first()
        val schemeEnd = cut.indexOf("://")
        if (schemeEnd < 0) return cut
        val scheme = cut.substring(0, schemeEnd)
        val rest = cut.substring(schemeEnd + 3)
        val slash = rest.indexOf('/')
        val host = if (slash >= 0) rest.substring(0, slash) else rest
        val path = if (slash >= 0) rest.substring(slash + 1) else ""
        val bareHost = host.substringAfterLast('@')
        return if (path.isEmpty()) "$scheme://$bareHost" else "$scheme://$bareHost/$path"
    }
}

/** Which Haven this is. */
object Haven {
    val version: String get() = runCatching { net.haven.app.BuildConfig.VERSION_NAME }.getOrDefault("?")
    val build: Int get() = runCatching { net.haven.app.BuildConfig.HAVEN_BUILD }.getOrDefault(0)
    val userAgent: String get() = "Haven/$version"
}
