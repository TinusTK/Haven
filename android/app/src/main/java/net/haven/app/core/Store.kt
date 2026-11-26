package net.haven.app.core

import androidx.sqlite.SQLiteConnection
import androidx.sqlite.SQLiteStatement
import androidx.sqlite.driver.bundled.BundledSQLiteDriver
import androidx.sqlite.driver.bundled.SQLITE_OPEN_CREATE
import androidx.sqlite.driver.bundled.SQLITE_OPEN_FULLMUTEX
import androidx.sqlite.driver.bundled.SQLITE_OPEN_READWRITE
import kotlinx.serialization.KSerializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.serializer
import java.io.File

/**
 * Haven's local SQLite database (`haven.sqlite3`).
 *
 * The schema is the Rust build's, step for step: the same tables, the same
 * `PRAGMA user_version` numbering, so a phone that updates from the Rust
 * build keeps everything, and each build refuses a database made by a newer
 * one. Never edit a step that has shipped: add a new one.
 *
 * SQLite comes with the app (androidx.sqlite's bundled driver): the tables
 * are STRICT, which Android's own SQLite only reads from Android 14 on.
 */
class Store private constructor(private val db: SQLiteConnection) : AutoCloseable {
    private val lock = Any()

    companion object {
        /** Schema steps, applied in order. The index + 1 is the schema version. */
        val migrations: List<String> = listOf(
            """
            CREATE TABLE kv (
                key   TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL
            ) STRICT;
            """,
            """
            CREATE TABLE addons (
                url      TEXT PRIMARY KEY NOT NULL,
                manifest TEXT NOT NULL,
                position INTEGER NOT NULL
            ) STRICT;
            """,
            """
            CREATE TABLE progress (
                kind       TEXT NOT NULL,
                title_id   TEXT NOT NULL,
                video_id   TEXT NOT NULL,
                title      TEXT NOT NULL,
                detail     TEXT NOT NULL,
                poster     TEXT,
                position   REAL NOT NULL,
                duration   REAL NOT NULL,
                finished   INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (kind, title_id, video_id)
            ) STRICT;
            CREATE INDEX progress_recent ON progress (updated_at DESC);
            """,
            """
            CREATE TABLE watchlist (
                kind     TEXT NOT NULL,
                title_id TEXT NOT NULL,
                title    TEXT NOT NULL,
                year     TEXT NOT NULL,
                poster   TEXT,
                added_at INTEGER NOT NULL,
                PRIMARY KEY (kind, title_id)
            ) STRICT;
            """,
            """
            CREATE TABLE tv_sources (
                url      TEXT PRIMARY KEY NOT NULL,
                name     TEXT NOT NULL,
                playlist TEXT NOT NULL,
                position INTEGER NOT NULL
            ) STRICT;
            """,
            """
            CREATE TABLE servers (
                id      INTEGER PRIMARY KEY AUTOINCREMENT,
                kind    TEXT NOT NULL,
                name    TEXT NOT NULL,
                address TEXT NOT NULL,
                user_id TEXT NOT NULL,
                token   TEXT NOT NULL,
                UNIQUE (kind, address, user_id)
            ) STRICT;
            """,
            """
            CREATE TABLE downloads (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                kind       TEXT NOT NULL,
                title_id   TEXT NOT NULL,
                video_id   TEXT NOT NULL,
                title      TEXT NOT NULL,
                detail     TEXT NOT NULL,
                season     INTEGER,
                episode    INTEGER,
                poster     TEXT,
                url        TEXT NOT NULL,
                path       TEXT NOT NULL,
                size       INTEGER NOT NULL,
                received   INTEGER NOT NULL,
                state      TEXT NOT NULL,
                added_at   INTEGER NOT NULL
            ) STRICT;
            """,
            """
            CREATE TABLE sync_queue (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                what       TEXT NOT NULL,
                kind       TEXT NOT NULL,
                title_id   TEXT NOT NULL,
                video_id   TEXT NOT NULL,
                created_at INTEGER NOT NULL
            ) STRICT;
            """,
            // Haven Hub is gone (0.2.4): forget any pairing with one.
            "DELETE FROM servers WHERE kind = 'hub';",
        )

        /** JSON as the Rust build wrote it: compact, every field, slashes
         *  not escaped; older saved values with missing fields still load. */
        val json = Json {
            encodeDefaults = true
            ignoreUnknownKeys = true
            explicitNulls = true
        }

        fun open(file: File): Store = guarded {
            file.parentFile?.mkdirs()
            val flags = SQLITE_OPEN_READWRITE or SQLITE_OPEN_CREATE or SQLITE_OPEN_FULLMUTEX
            Store(BundledSQLiteDriver().open(file.path, flags)).also { it.start() }
        }

        fun inMemory(): Store = guarded { Store(BundledSQLiteDriver().open(":memory:")).also { it.start() } }

        /** Turns the driver's errors into Haven's. */
        private inline fun <T> guarded(body: () -> T): T = try {
            body()
        } catch (e: HavenError) {
            throw e
        } catch (e: Exception) {
            throw HavenError.Database(e.message ?: e.toString())
        }

        /** The statements in one step (none of them has `;` inside). */
        fun statements(step: String): List<String> = step.split(";").map { it.trim() }.filter { it.isNotEmpty() }

        private const val PROGRESS_COLUMNS =
            "kind, title_id, video_id, title, detail, poster, position, duration, finished, updated_at"

        /** For each title, the row saved last (ties: the larger video id). */
        private const val LATEST_PER_TITLE = """
            NOT EXISTS (
                SELECT 1 FROM progress AS q
                WHERE q.kind = p.kind AND q.title_id = p.title_id
                  AND (q.updated_at > p.updated_at
                       OR (q.updated_at = p.updated_at AND q.video_id > p.video_id)))
            """

        private const val DOWNLOAD_COLUMNS =
            "id, kind, title_id, video_id, title, detail, season, episode, poster, url, path, size, received, state, added_at"
    }

    private fun start() {
        exec("PRAGMA foreign_keys = ON")
        migrate()
    }

    /** Brings the database up to the newest step, one step per transaction. */
    private fun migrate() {
        val known = migrations.size
        val found = schemaVersion()
        if (found > known) throw HavenError.DatabaseTooNew(found, known)
        for (index in found until known) {
            transaction {
                statements(migrations[index]).forEach { exec(it) }
                exec("PRAGMA user_version = ${index + 1}")
            }
        }
    }

    fun schemaVersion(): Int = query("PRAGMA user_version") { it.getLong(0).toInt() }.firstOrNull() ?: 0

    override fun close() = synchronized(lock) { db.close() }

    // MARK: Saved values (kv)

    /** A value saved with [put]; null if never saved. */
    inline fun <reified T> get(key: String): T? = get(key, serializer<T>())

    fun <T> get(key: String, serializer: KSerializer<T>): T? {
        val text = rawValue(key) ?: return null
        return try {
            json.decodeFromString(serializer, text)
        } catch (e: Exception) {
            throw HavenError.Json(e.message ?: e.toString())
        }
    }

    inline fun <reified T> put(key: String, value: T) = put(key, value, serializer<T>())

    fun <T> put(key: String, value: T, serializer: KSerializer<T>) {
        val text = try {
            json.encodeToString(serializer, value)
        } catch (e: Exception) {
            throw HavenError.Json(e.message ?: e.toString())
        }
        putRaw(key, text)
    }

    /** A saved value's JSON as it is stored (backups carry it as it is). */
    fun rawValue(key: String): String? = query("SELECT value FROM kv WHERE key = ?", key) { it.getText(0) }.firstOrNull()

    /** Saves JSON text as it is. */
    fun putRaw(key: String, text: String) =
        run("INSERT INTO kv (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value", key, text)

    fun remove(key: String) = run("DELETE FROM kv WHERE key = ?", key)

    // MARK: Add-ons

    /** Installed add-ons, in the order they were installed. */
    fun addons(): List<StoredAddon> =
        query("SELECT url, manifest FROM addons ORDER BY position, url") { StoredAddon(it.getText(0), it.getText(1)) }

    /** Adds an add-on at the end, or refreshes the manifest of one already
     *  installed (it keeps its place). */
    fun saveAddon(url: String, manifest: String) = run(
        """
        INSERT INTO addons (url, manifest, position)
        VALUES (?, ?, (SELECT COALESCE(MAX(position), 0) + 1 FROM addons))
        ON CONFLICT(url) DO UPDATE SET manifest = excluded.manifest
        """,
        url, manifest,
    )

    fun removeAddon(url: String) = run("DELETE FROM addons WHERE url = ?", url)

    // MARK: Progress

    private fun progressOf(s: SQLiteStatement) = WatchProgress(
        kind = s.getText(0), titleId = s.getText(1), videoId = s.getText(2), title = s.getText(3),
        detail = s.getText(4), poster = s.textOrNull(5), position = s.getDouble(6), duration = s.getDouble(7),
        finished = s.getLong(8) != 0L, updatedAt = s.getLong(9),
    )

    /** Saves how far a film or episode was watched, replacing what was saved
     *  for it before. A poster already on record is kept when the new entry
     *  has none. */
    fun saveProgress(p: WatchProgress) = run(
        """
        INSERT INTO progress ($PROGRESS_COLUMNS)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(kind, title_id, video_id) DO UPDATE SET
           title = excluded.title,
           detail = excluded.detail,
           poster = COALESCE(excluded.poster, poster),
           position = excluded.position,
           duration = excluded.duration,
           finished = excluded.finished,
           updated_at = excluded.updated_at
        """,
        p.kind, p.titleId, p.videoId, p.title, p.detail, p.poster, p.position, p.duration, p.finished, p.updatedAt,
    )

    /** Forgets one film or episode (marked unwatched). */
    fun removeVideoProgress(kind: String, titleId: String, videoId: String) =
        run("DELETE FROM progress WHERE kind = ? AND title_id = ? AND video_id = ?", kind, titleId, videoId)

    /** What was saved for one film or episode, if anything. */
    fun progress(kind: String, titleId: String, videoId: String): WatchProgress? = query(
        "SELECT $PROGRESS_COLUMNS FROM progress WHERE kind = ? AND title_id = ? AND video_id = ?",
        kind, titleId, videoId,
    ) { progressOf(it) }.firstOrNull()

    /** Titles to carry on with, most recent first: for each title the film
     *  or episode watched last, unless that one was finished. */
    fun continueWatching(limit: Int): List<WatchProgress> = query(
        """
        SELECT $PROGRESS_COLUMNS FROM progress AS p
        WHERE finished = 0 AND $LATEST_PER_TITLE
        ORDER BY updated_at DESC, title_id
        LIMIT ?
        """,
        limit,
    ) { progressOf(it) }

    /** Everything watched, newest first: for each title the film or episode
     *  watched last, finished or not. */
    fun history(limit: Int): List<WatchProgress> = query(
        """
        SELECT $PROGRESS_COLUMNS FROM progress AS p
        WHERE $LATEST_PER_TITLE
        ORDER BY updated_at DESC, title_id
        LIMIT ?
        """,
        limit,
    ) { progressOf(it) }

    /** Every film and episode with a position or a watched mark, newest first. */
    fun allProgress(): List<WatchProgress> =
        query("SELECT $PROGRESS_COLUMNS FROM progress ORDER BY updated_at DESC") { progressOf(it) }

    /** The films and episodes of one title watched to the end. */
    fun finishedVideos(kind: String, titleId: String): List<String> =
        query("SELECT video_id FROM progress WHERE kind = ? AND title_id = ? AND finished = 1", kind, titleId) {
            it.getText(0)
        }

    /** Records something watched elsewhere (brought in from a tracking
     *  service). A record Haven already has stays, unless it is partly
     *  watched and was last played before the service's watch: then it
     *  becomes finished. True if added or finished. */
    fun addWatchedElsewhere(p: WatchProgress): Boolean = changing(
        """
        INSERT INTO progress ($PROGRESS_COLUMNS)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, ?)
        ON CONFLICT (kind, title_id, video_id) DO UPDATE SET
            finished = 1, position = 0, updated_at = excluded.updated_at
        WHERE progress.finished = 0 AND excluded.updated_at > progress.updated_at
        """,
        p.kind, p.titleId, p.videoId, p.title, p.detail, p.poster, p.position, p.duration, p.updatedAt,
    ) > 0

    /** Forgets everything watched of one title (all its episodes). */
    fun removeProgress(kind: String, titleId: String) =
        run("DELETE FROM progress WHERE kind = ? AND title_id = ?", kind, titleId)

    // MARK: Media servers

    /** The media servers, in the order they were added. */
    fun servers(): List<StoredServer> =
        query("SELECT id, kind, name, address, user_id, token FROM servers ORDER BY id") {
            StoredServer(it.getLong(0), it.getText(1), it.getText(2), it.getText(3), it.getText(4), it.getText(5))
        }

    /** Adds a server, or refreshes the name and token of one already there
     *  (same kind, address and user). Returns its number. */
    fun saveServer(s: StoredServer): Long = query(
        """
        INSERT INTO servers (kind, name, address, user_id, token)
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT(kind, address, user_id) DO UPDATE SET
           name = excluded.name, token = excluded.token
        RETURNING id
        """,
        s.kind, s.name, s.address, s.userId, s.token,
    ) { it.getLong(0) }.firstOrNull() ?: 0

    fun removeServer(id: Long) = run("DELETE FROM servers WHERE id = ?", id)

    // MARK: Live TV sources

    /** Television and radio sources, in the order they were added. */
    fun tvSources(): List<TvSource> =
        query("SELECT url, name, playlist FROM tv_sources ORDER BY position, url") {
            TvSource(it.getText(0), it.getText(1), it.getText(2))
        }

    /** Adds a source at the end, or refreshes the playlist of one already
     *  there (it keeps its place and name). */
    fun saveTvSource(s: TvSource) = run(
        """
        INSERT INTO tv_sources (url, name, playlist, position)
        VALUES (?, ?, ?, (SELECT COALESCE(MAX(position), 0) + 1 FROM tv_sources))
        ON CONFLICT(url) DO UPDATE SET playlist = excluded.playlist
        """,
        s.url, s.name, s.playlist,
    )

    fun removeTvSource(url: String) = run("DELETE FROM tv_sources WHERE url = ?", url)

    // MARK: Watchlist

    /** The watchlist, newest first. */
    fun watchlist(): List<Saved> =
        query("SELECT kind, title_id, title, year, poster, added_at FROM watchlist ORDER BY added_at DESC, title") {
            Saved(it.getText(0), it.getText(1), it.getText(2), it.getText(3), it.textOrNull(4), it.getLong(5))
        }

    /** Saves a title to watch later. Saving it again keeps its place in the
     *  list and refreshes its name, year and poster. */
    fun addToWatchlist(s: Saved) = run(
        """
        INSERT INTO watchlist (kind, title_id, title, year, poster, added_at)
        VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(kind, title_id) DO UPDATE SET
           title = excluded.title,
           year = excluded.year,
           poster = COALESCE(excluded.poster, poster)
        """,
        s.kind, s.titleId, s.title, s.year, s.poster, s.addedAt,
    )

    fun removeFromWatchlist(kind: String, titleId: String) =
        run("DELETE FROM watchlist WHERE kind = ? AND title_id = ?", kind, titleId)

    fun inWatchlist(kind: String, titleId: String): Boolean =
        query("SELECT EXISTS (SELECT 1 FROM watchlist WHERE kind = ? AND title_id = ?)", kind, titleId) {
            it.getLong(0) != 0L
        }.firstOrNull() ?: false

    // MARK: Sync queue

    /** Notes a change to send. A Watchlist change cancels a waiting opposite
     *  one for the same title; watched / unwatched for the same video: the
     *  last one counts; the same change is kept once. */
    fun queueChange(c: Change) = transaction {
        when {
            c.what.startsWith("watchlist_") -> run(
                "DELETE FROM sync_queue WHERE what LIKE 'watchlist_%' AND kind = ? AND title_id = ?", c.kind, c.titleId,
            )
            c.what == "watched" || c.what == "unwatched" -> run(
                "DELETE FROM sync_queue WHERE what IN ('watched', 'unwatched') AND kind = ? AND title_id = ? AND video_id = ?",
                c.kind, c.titleId, c.videoId,
            )
            else -> run(
                "DELETE FROM sync_queue WHERE what = ? AND kind = ? AND title_id = ? AND video_id = ?",
                c.what, c.kind, c.titleId, c.videoId,
            )
        }
        run(
            "INSERT INTO sync_queue (what, kind, title_id, video_id, created_at) VALUES (?, ?, ?, ?, ?)",
            c.what, c.kind, c.titleId, c.videoId, c.createdAt,
        )
    }

    /** The changes still to send, oldest first. */
    fun queuedChanges(): List<Change> =
        query("SELECT id, what, kind, title_id, video_id, created_at FROM sync_queue ORDER BY id") {
            Change(it.getLong(0), it.getText(1), it.getText(2), it.getText(3), it.getText(4), it.getLong(5))
        }

    fun removeChange(id: Long) = run("DELETE FROM sync_queue WHERE id = ?", id)

    // MARK: Downloads

    private fun downloadOf(r: SQLiteStatement) = Download(
        id = r.getLong(0), kind = r.getText(1), titleId = r.getText(2), videoId = r.getText(3), title = r.getText(4),
        detail = r.getText(5), season = r.intOrNull(6), episode = r.intOrNull(7), poster = r.textOrNull(8),
        url = r.getText(9), path = r.getText(10), size = r.getLong(11), received = r.getLong(12),
        state = r.getText(13), addedAt = r.getLong(14),
    )

    /** Every download: films and series by name, episodes in order. */
    fun downloads(): List<Download> = query(
        """
        SELECT $DOWNLOAD_COLUMNS FROM downloads
        ORDER BY kind, title COLLATE NOCASE, title_id, season, episode, added_at
        """,
    ) { downloadOf(it) }

    /** Adds a download; returns its number. */
    fun addDownload(d: Download): Long = query(
        """
        INSERT INTO downloads (kind, title_id, video_id, title, detail, season, episode,
                               poster, url, path, size, received, state, added_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        RETURNING id
        """,
        d.kind, d.titleId, d.videoId, d.title, d.detail, d.season, d.episode,
        d.poster, d.url, d.path, d.size, d.received, d.state, d.addedAt,
    ) { it.getLong(0) }.firstOrNull() ?: 0

    /** Records how far a download has got, and its state. */
    fun updateDownload(id: Long, size: Long, received: Long, state: String) =
        run("UPDATE downloads SET size = ?, received = ?, state = ? WHERE id = ?", size, received, state, id)

    /** Changes where a download is saved (once its file type is known). */
    fun setDownloadPath(id: Long, path: String) = run("UPDATE downloads SET path = ? WHERE id = ?", path, id)

    fun removeDownload(id: Long) = run("DELETE FROM downloads WHERE id = ?", id)

    // MARK: Plumbing

    private fun SQLiteStatement.textOrNull(column: Int): String? = if (isNull(column)) null else getText(column)

    private fun SQLiteStatement.intOrNull(column: Int): Int? = if (isNull(column)) null else getLong(column).toInt()

    private fun SQLiteStatement.bindAll(args: Array<out Any?>) {
        args.forEachIndexed { i, value ->
            val at = i + 1
            when (value) {
                null -> bindNull(at)
                is String -> bindText(at, value)
                is Boolean -> bindLong(at, if (value) 1 else 0)
                is Int -> bindLong(at, value.toLong())
                is Long -> bindLong(at, value)
                is Double -> bindDouble(at, value)
                else -> error("cannot store ${value::class.simpleName}")
            }
        }
    }

    private fun exec(sql: String) = synchronized(lock) { guarded { db.prepare(sql).use { while (it.step()) Unit } } }

    private fun run(sql: String, vararg args: Any?) {
        changing(sql, *args)
    }

    /** Runs one statement; says how many rows it changed. */
    private fun changing(sql: String, vararg args: Any?): Int = synchronized(lock) {
        guarded {
            db.prepare(sql).use { s ->
                s.bindAll(args)
                while (s.step()) Unit
            }
            db.prepare("SELECT changes()").use { s -> if (s.step()) s.getLong(0).toInt() else 0 }
        }
    }

    private fun <T> query(sql: String, vararg args: Any?, row: (SQLiteStatement) -> T): List<T> = synchronized(lock) {
        guarded {
            db.prepare(sql).use { s ->
                s.bindAll(args)
                buildList { while (s.step()) add(row(s)) }
            }
        }
    }

    private fun transaction(body: () -> Unit) = synchronized(lock) {
        exec("BEGIN IMMEDIATE")
        try {
            body()
            exec("COMMIT")
        } catch (e: Exception) {
            runCatching { exec("ROLLBACK") }
            throw e
        }
    }
}
