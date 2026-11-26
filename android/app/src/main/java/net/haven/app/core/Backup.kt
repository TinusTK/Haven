package net.haven.app.core

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.longOrNull
import net.haven.app.tracking.TrackedAccount
import java.io.File
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

/** A backup of Haven: the whole setup (media servers and their sign-ins,
 *  add-ons in builds that have them, Trakt and Simkl, keys, TV sources, the
 *  Watchlist, what was watched, what is switched off) and the settings, as
 *  one JSON file, `Haven backup.json`. The same file as the Rust build's
 *  (`haven-core` `backup.rs` and `setup.rs`) and the iPhone's, so a backup
 *  moves between the old app, the new one and other devices. Restoring adds
 *  what is missing and never removes anything. */
object Backup {
    const val FILE_NAME = "Haven backup.json"
    const val KIND = "Haven backup"
    /** Settings that travel with a backup (others belong to one device). */
    val settings = listOf("settings", "sync_options", "external_player", "country", "tv_favourites", "player", "update_check_at_start")
    /** When the last backup was made, in seconds since 1970. */
    const val LAST_KEY = "backup_last"
    /** How often the backup is brought up to date while Haven is open, and
     *  how long the first one waits after start. */
    const val EVERY_MILLIS = 10 * 60 * 1000L
    const val AFTER_START_MILLIS = 30 * 1000L

    const val FIRST_NOTE = "Haven backs up your media servers, accounts, Trakt and Simkl, keys, Watchlist, what you watched and your settings while it is open."
    /** Where the backup goes, for its row (the Rust build's words). */
    const val WHERE_WORDS = "On this phone"

    /** What a restore added. */
    data class Merged(
        val added: Int = 0,
        /** A Trakt or Simkl sign-in was added or replaced by a newer one. */
        val tracking: Boolean = false,
    )

    class NotABackup : Exception("That file is not a Haven backup")

    private val json = Json { prettyPrint = true }
    private val plain = Json

    // Making one

    /** The backup of `store`, made at `now` (seconds since 1970). `sources`:
     *  add-ons go in (only builds that have the Add-ons page). */
    fun make(store: Store, sources: Boolean, now: Long): JsonObject {
        val kept = LinkedHashMap<String, JsonElement>()
        for (key in settings) store.rawValue(key)?.let(::parse)?.let { kept[key] = it }
        return JsonObject(
            mapOf(
                "kind" to JsonPrimitive(KIND),
                "version" to JsonPrimitive(1),
                "made_at" to JsonPrimitive(now),
                "setup" to setup(store, sources),
                "settings" to JsonObject(kept),
            ),
        )
    }

    private fun text(value: String) = JsonPrimitive(value)
    private fun obj(vararg pairs: Pair<String, JsonElement>) = JsonObject(mapOf(*pairs))

    /** The setup part (what `Setup::read` gives in the Rust build). */
    fun setup(store: Store, sources: Boolean): JsonObject {
        val off = SwitchedOff.load(store)
        val servers = store.servers()
        val addons = if (sources) store.addons().map { obj("url" to text(it.url), "description" to text(it.manifest)) } else emptyList()
        val keys = Keys.load(store)
        val tracking = store.rawValue(TrackedAccount.KEY)?.let(::parse) as? JsonArray ?: JsonArray(emptyList())
        return obj(
            "servers" to JsonArray(servers.map {
                obj("kind" to text(it.kind), "name" to text(it.name), "address" to text(it.address), "user_id" to text(it.userId), "token" to text(it.token))
            }),
            "sources" to JsonArray(addons),
            "tv_sources" to JsonArray(store.tvSources().map { obj("url" to text(it.url), "name" to text(it.name)) }),
            "keys" to obj(
                "tmdb" to text(keys.tmdb), "trakt_id" to text(keys.traktId), "trakt_secret" to text(keys.traktSecret),
                "simkl_id" to text(keys.simklId), "omdb" to text(keys.omdb),
            ),
            "watchlist" to JsonArray(store.watchlist().map {
                obj(
                    "kind" to text(it.kind), "title_id" to text(it.titleId), "title" to text(it.title), "year" to text(it.year),
                    "poster" to (it.poster?.let(::text) ?: JsonNull), "added_at" to JsonPrimitive(it.addedAt),
                )
            }),
            "progress" to JsonArray(store.allProgress().map {
                obj(
                    "kind" to text(it.kind), "title_id" to text(it.titleId), "video_id" to text(it.videoId), "title" to text(it.title),
                    "detail" to text(it.detail), "poster" to (it.poster?.let(::text) ?: JsonNull), "position" to JsonPrimitive(it.position),
                    "duration" to JsonPrimitive(it.duration), "finished" to JsonPrimitive(it.finished), "updated_at" to JsonPrimitive(it.updatedAt),
                )
            }),
            "tracking" to tracking,
            "off" to obj(
                "servers" to JsonArray(servers.filter { it.id in off.servers }.map { obj("kind" to text(it.kind), "address" to text(it.address)) }),
                "addons" to JsonArray(if (sources) off.addons.map(::text) else emptyList()),
                "tv_sources" to JsonArray(off.tvSources.map(::text)),
            ),
        )
    }

    /** Every object's keys in order, so the same backup always reads the same. */
    private fun sorted(value: JsonElement): JsonElement = when (value) {
        is JsonObject -> JsonObject(value.toSortedMap().mapValues { sorted(it.value) })
        is JsonArray -> JsonArray(value.map(::sorted))
        else -> value
    }

    /** The text written to the file. */
    fun text(backup: JsonObject): String = json.encodeToString(JsonElement.serializer(), sorted(backup))

    /** What decides whether the backup changed: the setup and settings,
     *  without the time it was made. */
    fun contents(backup: JsonObject): String = plain.encodeToString(
        JsonElement.serializer(),
        sorted(obj("setup" to (backup["setup"] ?: JsonObject(emptyMap())), "settings" to (backup["settings"] ?: JsonObject(emptyMap())))),
    )

    /** Writes it to `file`, replacing the old one only once the new one is
     *  complete (a crash halfway leaves the old backup whole). */
    fun write(text: String, file: File) {
        file.parentFile?.mkdirs()
        val partial = File(file.parentFile, file.name.removeSuffix(".json") + ".json.part")
        partial.writeText(text)
        if (!partial.renameTo(file)) {
            file.delete()
            if (!partial.renameTo(file)) throw java.io.IOException("could not write ${file.name}")
        }
    }

    // Reading one

    /** JSON text, or null. kotlinx reads a bare word as a string, which
     *  JSON does not allow: those are refused. */
    private fun parse(text: String): JsonElement? {
        val trimmed = text.trim()
        val start = trimmed.firstOrNull() ?: return null
        if (start !in "{[\"-0123456789" && trimmed != "true" && trimmed != "false" && trimmed != "null") return null
        return runCatching { plain.parseToJsonElement(trimmed) }.getOrNull()
    }

    /** Reads a backup's text; fails if it is not a Haven backup. */
    fun read(text: String): JsonObject {
        val backup = parse(text) as? JsonObject ?: throw NotABackup()
        if (str(backup["kind"]) != KIND) throw NotABackup()
        return backup
    }

    /** When a backup was made, in seconds since 1970. */
    fun madeAt(backup: JsonObject): Long = whole(backup["made_at"])

    // Restoring

    /** Adds a backup to `store`. Settings are put back only where the store
     *  has none of its own yet (a fresh install). Nothing is removed. */
    fun restore(backup: JsonObject, store: Store, sources: Boolean): Merged {
        (backup["settings"] as? JsonObject)?.forEach { (key, value) ->
            if (key !in settings || store.rawValue(key) != null) return@forEach
            store.putRaw(key, plain.encodeToString(JsonElement.serializer(), value))
        }
        return merge(backup["setup"] as? JsonObject ?: JsonObject(emptyMap()), store, sources)
    }

    private fun str(value: JsonElement?): String = (value as? JsonPrimitive)?.takeIf { it.isString }?.content ?: ""
    private fun optional(value: JsonElement?): String? = (value as? JsonPrimitive)?.takeIf { it.isString }?.content
    private fun whole(value: JsonElement?): Long = (value as? JsonPrimitive)?.takeIf { !it.isString }?.let { it.longOrNull ?: it.doubleOrNull?.toLong() } ?: 0
    private fun real(value: JsonElement?): Double = (value as? JsonPrimitive)?.takeIf { !it.isString }?.doubleOrNull ?: 0.0
    private fun list(value: JsonElement?): List<JsonObject> = (value as? JsonArray)?.mapNotNull { it as? JsonObject } ?: emptyList()
    private fun words(value: JsonElement?): List<String> = (value as? JsonArray)?.mapNotNull { (it as? JsonPrimitive)?.contentOrNull } ?: emptyList()

    /** What `Setup::merge_into` does in the Rust build: servers, add-ons and
     *  TV sources already here by address are kept; what arrives switched
     *  off stays off; keys only fill in what is blank; Watchlist entries are
     *  added; for progress the newer record wins; a Trakt or Simkl sign-in
     *  is taken where there is none here, or an older one. */
    fun merge(setup: JsonObject, store: Store, sources: Boolean): Merged {
        var added = 0
        var tracking = false
        val offRecord = setup["off"] as? JsonObject ?: JsonObject(emptyMap())
        val offServers = list(offRecord["servers"]).map { str(it["kind"]) to str(it["address"]) }
        val offAddons = words(offRecord["addons"])
        val offTV = words(offRecord["tv_sources"])
        var off = SwitchedOff.load(store)
        var offChanged = false

        val have = store.servers()
        for (server in list(setup["servers"])) {
            val kind = str(server["kind"])
            val address = str(server["address"])
            if (have.any { it.address == address && it.kind == kind }) continue
            val id = store.saveServer(StoredServer(kind = kind, name = str(server["name"]), address = address, userId = str(server["user_id"]), token = str(server["token"])))
            if ((kind to address) in offServers) {
                off = off.copy(servers = off.servers + id)
                offChanged = true
            }
            added += 1
        }
        if (sources) {
            val installed = store.addons()
            for (addon in list(setup["sources"])) {
                val url = str(addon["url"])
                if (installed.any { it.url == url }) continue
                store.saveAddon(url, str(addon["description"]))
                if (url in offAddons && url !in off.addons) {
                    off = off.copy(addons = off.addons + url)
                    offChanged = true
                }
                added += 1
            }
        }
        val tv = store.tvSources()
        for (source in list(setup["tv_sources"])) {
            val url = str(source["url"])
            if (tv.any { it.url == url }) continue
            store.saveTvSource(TvSource(url, str(source["name"]), ""))
            if (url in offTV && url !in off.tvSources) {
                off = off.copy(tvSources = off.tvSources + url)
                offChanged = true
            }
            added += 1
        }
        if (offChanged) off.save(store)

        (setup["keys"] as? JsonObject)?.let { keys ->
            fun fill(mine: String, theirs: JsonElement?): String {
                val other = str(theirs)
                return if (mine.isBlank() && other.isNotBlank()) other else mine
            }
            val mine = Keys.load(store)
            mine.copy(
                tmdb = fill(mine.tmdb, keys["tmdb"]),
                traktId = fill(mine.traktId, keys["trakt_id"]),
                traktSecret = fill(mine.traktSecret, keys["trakt_secret"]),
                simklId = fill(mine.simklId, keys["simkl_id"]),
                omdb = fill(mine.omdb, keys["omdb"]),
            ).save(store)
        }
        for (saved in list(setup["watchlist"])) {
            val kind = str(saved["kind"])
            val id = str(saved["title_id"])
            if (store.inWatchlist(kind, id)) continue
            store.addToWatchlist(Saved(kind, id, str(saved["title"]), str(saved["year"]), optional(saved["poster"]), whole(saved["added_at"])))
            added += 1
        }
        for (record in list(setup["progress"])) {
            val progress = WatchProgress(
                kind = str(record["kind"]), titleId = str(record["title_id"]), videoId = str(record["video_id"]),
                title = str(record["title"]), detail = str(record["detail"]), poster = optional(record["poster"]),
                position = real(record["position"]), duration = real(record["duration"]),
                finished = (record["finished"] as? JsonPrimitive)?.booleanOrNull ?: false, updatedAt = whole(record["updated_at"]),
            )
            val mine = store.progress(progress.kind, progress.titleId, progress.videoId)
            if (mine != null && progress.updatedAt <= mine.updatedAt) continue
            store.saveProgress(progress)
            added += 1
        }
        val theirs = list(setup["tracking"])
        if (theirs.isNotEmpty()) {
            val mine = (store.rawValue(TrackedAccount.KEY)?.let(::parse) as? JsonArray)?.mapNotNull { it as? JsonObject }?.toMutableList() ?: mutableListOf()
            for (account in theirs) {
                val name = (account["service"] as? JsonPrimitive)?.contentOrNull ?: continue
                val at = mine.indexOfFirst { str(it["service"]) == name }
                if (at >= 0) {
                    if (whole(account["expires_at"]) > whole(mine[at]["expires_at"])) {
                        mine[at] = account
                        tracking = true
                    }
                } else {
                    mine += account
                    tracking = true
                    added += 1
                }
            }
            if (tracking) store.putRaw(TrackedAccount.KEY, plain.encodeToString(JsonElement.serializer(), JsonArray(mine)))
        }
        return Merged(added, tracking)
    }

    // Where it goes

    /** The folder and how to say where it is, as the Rust build: Documents ›
     *  Haven on the phone, else Haven's own folder (Android › data). */
    fun folder(documents: File?, own: File): Pair<File, String> {
        if (documents != null) {
            val shared = File(documents, "Haven")
            if ((shared.isDirectory || shared.mkdirs()) && shared.canWrite()) return shared to "Documents › Haven on this phone"
        }
        return own to "Android › data › net.haven.app on this phone"
    }

    /** "today at 21:40", or "3 Nov at 09:15". */
    fun `when`(at: Long, now: Instant = Instant.now(), zone: ZoneId = ZoneId.systemDefault()): String {
        val time = Instant.ofEpochSecond(at).atZone(zone)
        return if (time.toLocalDate() == now.atZone(zone).toLocalDate()) {
            "today at ${time.format(DateTimeFormatter.ofPattern("HH:mm", Locale.ENGLISH))}"
        } else {
            time.format(DateTimeFormatter.ofPattern("d MMM 'at' HH:mm", Locale.ENGLISH))
        }
    }
}
