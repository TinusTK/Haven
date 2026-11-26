package net.haven.app.core

import android.content.Context
import java.io.File

/**
 * The folder Haven keeps its files in: the app's own files folder
 * (`Context.filesDir`), where the Rust build kept them too
 * (`AndroidApp::internal_data_path`), so an update finds the old database,
 * log and secrets file.
 */
class AppPaths private constructor(val dataDir: File) {
    val database: File get() = File(dataDir, "haven.sqlite3")
    val logFile: File get() = File(dataDir, "haven.log")
    val secretsFile: File get() = File(dataDir, "secrets.env")
    /** Pictures kept between runs (cleared from Settings). */
    val pictures: File get() = File(dataDir, "cache/pictures")

    companion object {
        fun of(context: Context): AppPaths = at(context.filesDir)

        /** A given folder, created if missing (also used by tests). */
        fun at(folder: File): AppPaths {
            if (!folder.isDirectory && !folder.mkdirs()) throw HavenError.Io(folder.path, "could not make the folder")
            return AppPaths(folder)
        }
    }
}

/** Identifies this copy of Haven to media servers (kv `device_id`, made once
 *  and kept, `haven-` and 32 hex digits). An update from the Rust build keeps
 *  the same one, so servers see the same device. */
object DeviceId {
    const val KEY = "device_id"

    fun get(store: Store): String {
        runCatching { store.get<String>(KEY) }.getOrNull()?.takeIf { it.isNotEmpty() }?.let { return it }
        val bytes = ByteArray(16).also { java.security.SecureRandom().nextBytes(it) }
        val id = "haven-" + bytes.joinToString("") { "%02x".format(it) }
        runCatching { store.put(KEY, id) }.onFailure { Log.note("could not save the device id: ${it.message}") }
        return id
    }
}
