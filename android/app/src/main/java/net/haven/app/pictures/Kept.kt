package net.haven.app.pictures

import kotlinx.serialization.KSerializer
import kotlinx.serialization.Serializable
import kotlinx.serialization.serializer
import net.haven.app.core.AppPaths
import net.haven.app.core.Http
import net.haven.app.core.Log
import net.haven.app.core.Store
import net.haven.app.metadata.Omdb
import net.haven.app.servers.Server
import net.haven.app.servers.ServerAccounts
import java.io.File
import java.security.MessageDigest

/** Drops answers that come too late: each board or page (Home's shelves,
 *  Search, a title's page, Explore…) counts how often it was started afresh,
 *  and an answer is used only if it belongs to the latest start. */
class Generation {
    @Volatile var current: Long = 0
        private set

    /** Starts afresh: answers to anything earlier no longer count. */
    @Synchronized
    fun next(): Long {
        current += 1
        return current
    }

    /** Whether an answer started at `token` still counts. */
    fun isCurrent(token: Long) = token == current
}

/** Where Haven's cache lives: Android's cache folder for the app (left out
 *  of backups, and emptied by Android when space runs short). */
object Cache {
    /** A short file name for any text: the first 16 bytes of its SHA-256. */
    fun hashName(text: String): String =
        MessageDigest.getInstance("SHA-256").digest(text.toByteArray()).take(16).joinToString("") { "%02x".format(it) }

    fun pictures(cacheDir: File) = File(cacheDir, "Haven/pictures")

    fun kept(cacheDir: File) = Kept(File(cacheDir, "Haven/kept"))

    /** The Rust build kept its pictures and answers in the data folder
     *  (`cache/`). They are only a cache: removed once. */
    fun removeRustCache(paths: AppPaths) {
        val old = File(paths.dataDir, "cache")
        if (old.exists()) {
            old.deleteRecursively()
            Log.note("removed the old picture cache from the data folder")
        }
    }
}

/** What Haven loaded last time, kept on disk so the next start shows it at
 *  once: Home's shelves, Explore's first pages, the media servers' libraries,
 *  titles' details and OMDb's scores. Each is asked for again in the
 *  background and replaced when the fresh answer differs; a fresh answer that
 *  fails keeps what was kept, and no error shows. */
class Kept(val dir: File) {
    /** The file a kept answer lives in, named after what it is. */
    fun file(name: String) = File(dir, Cache.hashName(name) + ".json")

    /** A kept answer; anything missing or unreadable is null. */
    inline fun <reified T> read(name: String): T? = read(name, serializer<T>())

    fun <T> read(name: String, serializer: KSerializer<T>): T? =
        runCatching { Store.json.decodeFromString(serializer, file(name).readText()) }.getOrNull()

    /** Writes a kept answer (whole or not at all). */
    inline fun <reified T> write(name: String, value: T) = write(name, value, serializer<T>())

    fun <T> write(name: String, value: T, serializer: KSerializer<T>) {
        val text = runCatching { Store.json.encodeToString(serializer, value) }.getOrNull() ?: return
        runCatching {
            dir.mkdirs()
            val target = file(name)
            val part = File(dir, target.name + ".part")
            part.writeText(text)
            if (!part.renameTo(target)) {
                target.delete()
                part.renameTo(target)
            }
        }
    }

    /** Keeps a fresh answer. True when it differs from what was kept (what is
     *  on screen should then be replaced). */
    inline fun <reified T> freshen(name: String, fresh: T): Boolean {
        if (read<T>(name) == fresh) return false
        write(name, fresh)
        return true
    }

    /** Forgets everything kept (Settings' Clear, with the pictures). */
    fun clear() {
        dir.deleteRecursively()
    }

    companion object {
        // Names, as the Rust build named them
        fun shelf(board: String, title: String, source: String) = "shelf|$board|$title|$source"
        fun explore(label: String, kind: String, genre: String) = "explore|$label|$kind|$genre"
        fun libraries(server: Server) = ServerAccounts.librariesName(server)
        fun details(kind: String, id: String) = "details|$kind|$id"
        fun omdb(imdb: String) = "omdb|$imdb"

        /** OMDb scores are kept a week (the free key allows 1,000 a day). */
        const val OMDB_KEPT_FOR = 7 * 86_400L
    }

    @Serializable
    data class KeptScores(
        /** When they were fetched, in seconds since 1970. */
        val at: Long,
        val scores: Omdb.Scores,
    )

    /** A title's OMDb scores: kept ones while under a week old, else fresh ones
     *  (then kept); the kept ones when fresh ones cannot be had. */
    suspend fun scores(imdb: String, key: String?, now: Long, http: Http = Http.shared): Omdb.Scores? {
        val kept = read<KeptScores>(omdb(imdb))
        if (kept != null && now - kept.at < OMDB_KEPT_FOR) return kept.scores
        if (key == null) return kept?.scores
        val found = runCatching { Omdb.scores(key, imdb, http) }.getOrNull() ?: return kept?.scores
        write(omdb(imdb), KeptScores(now, found))
        return found
    }
}
