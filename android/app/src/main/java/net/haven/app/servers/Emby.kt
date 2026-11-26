package net.haven.app.servers

import kotlinx.coroutines.delay
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject
import kotlinx.serialization.json.addJsonObject
import net.haven.app.core.Haven
import net.haven.app.core.HttpError
import net.haven.app.core.Log
import net.haven.app.titles.MediaStream
import net.haven.app.titles.MetaDetail
import net.haven.app.titles.MetaPreview
import net.haven.app.titles.Video
import java.io.ByteArrayOutputStream
import java.net.SocketTimeoutException

/** An add-on a server uses and is willing to tell its owner about. Its
 *  manifest address can hold the user's keys for other services, so it is
 *  never printed. */
data class SharedAddon(val name: String, val manifestUrl: String) {
    override fun toString(): String = "SharedAddon(name=$name, ..)"
}

/** Picks the entries of `MediaSources` out of an answer while it is still
 *  arriving, each as soon as its last byte is in. */
class SourceScanner {
    companion object {
        /** The most one entry may take (one with more is cut short and fails to read). */
        const val ENTRY_BYTES = 4 * 1024 * 1024
        private val MEDIA_SOURCES = "MediaSources".toByteArray()
    }

    private var depth = 0
    private var inString = false
    private var escape = false
    /** The last string seen at the top level: the key before a value. */
    private val key = ByteArrayOutputStream()
    private var inList = false
    /** The entry being gathered, if inside one. */
    private var entry: ByteArrayOutputStream? = null
    /** The list has ended. */
    var done = false
        private set

    /** Takes one byte; gives an entry once one is complete. */
    fun push(byte: Byte): JsonElement? {
        if (done) return null
        entry?.let { if (it.size() < ENTRY_BYTES) it.write(byte.toInt()) }
        val top = depth == 1 && !inList
        val c = byte.toInt().toChar()
        if (inString) {
            when {
                escape -> escape = false
                c == '\\' -> escape = true
                c == '"' -> inString = false
                top -> key.write(byte.toInt())
            }
            return null
        }
        when (c) {
            '"' -> {
                inString = true
                if (top) key.reset()
            }
            '{', '[' -> {
                if (c == '[' && top && key.toByteArray().contentEquals(MEDIA_SOURCES)) {
                    inList = true
                } else if (c == '{' && inList && depth == 2) {
                    entry = ByteArrayOutputStream().also { it.write(byte.toInt()) }
                }
                depth += 1
            }
            '}', ']' -> {
                depth = maxOf(depth - 1, 0)
                if (inList && depth == 2) {
                    val finished = entry
                    if (finished != null) {
                        entry = null
                        return runCatching { Json.parseToJsonElement(finished.toString(Charsets.UTF_8)) }.getOrNull()
                    }
                } else if (inList && depth == 1) {
                    inList = false
                    done = true
                }
            }
        }
        return null
    }

    /** Takes a piece of the answer; gives the entries it completed. */
    fun feed(part: ByteArray, from: Int = 0, to: Int = part.size): List<JsonElement> {
        val entries = mutableListOf<JsonElement>()
        for (i in from until to) {
            if (done) break
            push(part[i])?.let { entries += it }
        }
        return entries
    }
}

/** Jellyfin and Emby. Jellyfin grew out of Emby and the two still share the
 *  parts of the API used here; they differ in the sign-in header. */
object Emby {
    /** Collections listed per server (each becomes a shelf). */
    const val COLLECTIONS = 60
    /** One tick is 100 nanoseconds. */
    const val TICKS_PER_SECOND = 10_000_000.0

    fun headers(device: String, kind: ServerKind, token: String?): List<Pair<String, String>> {
        var value = "MediaBrowser Client=\"Haven\", Device=\"Haven\", DeviceId=\"$device\", Version=\"${Haven.version}\""
        if (token != null) value += ", Token=\"$token\""
        return when {
            kind == ServerKind.Emby && token != null -> listOf("X-Emby-Authorization" to value, "X-Emby-Token" to token)
            kind == ServerKind.Emby -> listOf("X-Emby-Authorization" to value)
            else -> listOf("Authorization" to value)
        }
    }

    suspend fun get(client: ServerClient, server: Server, path: String): JsonElement =
        client.call(server.address + path, headers(client.device, server.kind, server.token))

    private fun q(text: String) = Servers.query(text)

    suspend fun signIn(client: ServerClient, kind: ServerKind, address: String, user: String, password: String): Server {
        val answer = client.call(
            "$address/Users/AuthenticateByName", headers(client.device, kind, null),
            buildJsonObject { put("Username", user); put("Pw", password) },
        )
        val token = SJ.text(answer, "AccessToken") ?: throw ServerError.BadAnswer()
        val userId = SJ.text(SJ.obj(answer, "User"), "Id") ?: throw ServerError.BadAnswer()
        // The server's own name, if it will say; otherwise its address.
        val info = runCatching { client.call("$address/System/Info/Public", headers(client.device, kind, null)) }.getOrNull()
        val name = SJ.text(info, "ServerName") ?: address
        return Server(kind = kind, name = name, address = address, userId = userId, token = token)
    }

    suspend fun libraries(client: ServerClient, server: Server): List<MediaLibrary> {
        val views = get(client, server, "/Users/${server.userId}/Views")
        val libraries = readLibraries(views).toMutableList()
        // The collections view holds lists, not titles: each list is offered as
        // a library of its own (on a server that gathers catalogues, the catalogues).
        for (view in collectionViews(views)) {
            val path = "/Users/${server.userId}/Items?ParentId=${q(view)}&StartIndex=0&Limit=$COLLECTIONS"
            runCatching { get(client, server, path) }.getOrNull()?.let { libraries += readCollections(it) }
        }
        return libraries
    }

    fun collectionViews(views: JsonElement): List<String> =
        SJ.list(views, "Items").filter { SJ.text(it, "CollectionType") == "boxsets" }.mapNotNull { SJ.text(it, "Id") }

    fun readCollections(found: JsonElement): List<MediaLibrary> =
        SJ.list(found, "Items").filter { SJ.text(it, "Type") == "BoxSet" }.mapNotNull { item ->
            val id = SJ.text(item, "Id") ?: return@mapNotNull null
            val name = SJ.text(item, "Name") ?: return@mapNotNull null
            MediaLibrary(id, name, collection = true)
        }

    fun readLibraries(views: JsonElement): List<MediaLibrary> =
        SJ.list(views, "Items").filter { view ->
            // No type means a mixed library; other types (music, books) are skipped.
            val type = SJ.text(view, "CollectionType")
            type == null || type == "movies" || type == "tvshows"
        }.mapNotNull { view ->
            val id = SJ.text(view, "Id") ?: return@mapNotNull null
            val name = SJ.text(view, "Name") ?: return@mapNotNull null
            MediaLibrary(id, name, collection = false)
        }

    suspend fun genres(client: ServerClient, server: Server, library: String): List<String> {
        val filters = runCatching { get(client, server, "/Items/Filters?UserId=${server.userId}&ParentId=${q(library)}") }
            .getOrNull() ?: return emptyList()
        return SJ.list(filters, "Genres").mapNotNull {
            (it as? JsonPrimitive)?.takeIf { p -> p.isString }?.content?.trim()?.takeIf { t -> t.isNotEmpty() }
        }
    }

    suspend fun latest(client: ServerClient, server: Server, library: String): List<MetaPreview> {
        val path = "/Users/${server.userId}/Items/Latest?ParentId=${q(library)}&Limit=30&IncludeItemTypes=Movie,Series,Episode"
        return readPreviews(server, (get(client, server, path) as? JsonArray) ?: emptyList())
    }

    suspend fun page(client: ServerClient, server: Server, library: String, genre: String?, start: Int, length: Int): List<MetaPreview> {
        var path = "/Users/${server.userId}/Items?ParentId=${q(library)}&Recursive=true&IncludeItemTypes=Movie,Series" +
            "&SortBy=SortName&SortOrder=Ascending&StartIndex=$start&Limit=$length"
        if (genre != null) path += "&Genres=${q(genre)}"
        return readPreviews(server, SJ.list(get(client, server, path), "Items"))
    }

    suspend fun search(client: ServerClient, server: Server, words: String): List<MetaPreview> {
        val path = "/Users/${server.userId}/Items?SearchTerm=${q(words)}&Recursive=true&IncludeItemTypes=Movie,Series&Limit=30"
        return readPreviews(server, SJ.list(get(client, server, path), "Items"))
    }

    suspend fun online(client: ServerClient, server: Server): Boolean =
        runCatching { client.call("${server.address}/System/Info/Public", headers(client.device, server.kind, null)) }.isSuccess

    /** A title's ids at IMDb and TMDB, as the server recorded them. The keys'
     *  case differs between servers and versions. */
    fun providerIds(item: JsonElement?): Pair<String?, String?> {
        val ids = SJ.obj(item, "ProviderIds") ?: JsonObject(emptyMap())
        fun id(name: String): String? {
            val value = ids.entries.firstOrNull { it.key.equals(name, ignoreCase = true) }?.value as? JsonPrimitive ?: return null
            if (!value.isString) return null
            return value.content.trim().takeIf { it.isNotEmpty() }
        }
        return id("Imdb") to id("Tmdb")
    }

    fun readCandidates(items: JsonElement): List<Candidate> = SJ.list(items, "Items").mapNotNull { item ->
        val id = SJ.text(item, "Id") ?: return@mapNotNull null
        val name = SJ.text(item, "Name") ?: return@mapNotNull null
        val (imdb, tmdb) = providerIds(item)
        Candidate(id, name, SJ.text(item, "ProductionYear"), imdb, tmdb)
    }

    /** The id of the episode with that season and number, from a series' episode list. */
    fun readEpisode(episodes: JsonElement, season: Int, number: Int): String? =
        SJ.list(episodes, "Items").firstOrNull {
            SJ.number(it, "ParentIndexNumber") == season.toDouble() && SJ.number(it, "IndexNumber") == number.toDouble()
        }?.let { SJ.text(it, "Id") }

    suspend fun find(client: ServerClient, server: Server, wanted: Wanted): String? {
        val name = wanted.name.trim()
        if (name.isEmpty()) return null
        val path = "/Users/${server.userId}/Items?SearchTerm=${q(name)}&Recursive=true" +
            "&IncludeItemTypes=${if (wanted.series) "Series" else "Movie"}&Fields=ProviderIds&Limit=25"
        val match = Servers.bestMatch(wanted, readCandidates(get(client, server, path))) ?: return null
        // A gathered title listed under another number (TMDB, say) is asked for
        // under its IMDb id, which is what finds its streams.
        val found = wanted.imdb?.let { Servers.byImdb(match.id, it) } ?: match.id
        val episode = wanted.episode ?: return if (wanted.series) null else found
        val episodes = get(client, server, "/Shows/${q(found)}/Episodes?UserId=${server.userId}")
        return readEpisode(episodes, episode.first, episode.second)
    }

    /** The address of one of an item's pictures. The token is included
     *  because some servers only hand pictures to a signed-in app. */
    fun image(server: Server, item: String, which: String, height: Int, tag: String?): String {
        var address = "${server.address}/Items/${q(item)}/Images/$which?maxHeight=$height&quality=90"
        if (tag != null) address += "&tag=${q(tag)}"
        return address + "&api_key=${q(server.token)}"
    }

    fun posterTag(item: JsonElement?): String? = SJ.text(SJ.obj(item, "ImageTags"), "Primary")

    /** A picture the item names by its full web address. */
    fun remoteImage(item: JsonElement?, kind: String): String? =
        SJ.list(item, "RemoteImages").firstOrNull { SJ.text(it, "Type") == kind }
            ?.let { SJ.text(it, "Url") }
            ?.takeIf { it.startsWith("http://") || it.startsWith("https://") }

    /** Films and series as posters. An episode stands for its series; a
     *  series shows once however many of its episodes are listed. */
    fun readPreviews(server: Server, items: List<JsonElement>): List<MetaPreview> {
        val previews = mutableListOf<MetaPreview>()
        for (item in items) {
            val (kind, id, name, tag) = when (SJ.text(item, "Type")) {
                "Movie" -> listOf("movie", SJ.text(item, "Id"), SJ.text(item, "Name"), posterTag(item))
                "Series" -> listOf("series", SJ.text(item, "Id"), SJ.text(item, "Name"), posterTag(item))
                "Episode" -> listOf("series", SJ.text(item, "SeriesId"), SJ.text(item, "SeriesName"), SJ.text(item, "SeriesPrimaryImageTag"))
                else -> continue
            }
            if (id == null || name == null || isErrorEntry(id)) continue
            val full = Servers.titleId(server.id, id)
            if (previews.any { it.id == full }) continue
            previews += MetaPreview(
                id = full, kind = kind!!, name = name,
                poster = image(server, id, "Primary", 450, tag),
                releaseInfo = SJ.text(item, "ProductionYear"),
                rating = SJ.number(item, "CommunityRating")?.takeIf { it > 0 }?.let { SJ.rating(it) },
            )
        }
        return previews
    }

    /** A server that gathers catalogues passes on a source's error (such as
     *  "Rate Limit Exceeded") as if it were a film. */
    fun isErrorEntry(id: String): Boolean {
        val spelt = Servers.unhex(id) ?: return false
        val parts = spelt.split("|")
        return parts.size > 1 && parts[1].contains("streamserror.")
    }

    suspend fun meta(client: ServerClient, server: Server, item: String): MetaDetail {
        val detail = get(client, server, "/Users/${server.userId}/Items/${q(item)}")
        val episodes: JsonElement = if (SJ.text(detail, "Type") == "Series") {
            get(client, server, "/Shows/${q(item)}/Episodes?UserId=${server.userId}")
        } else {
            JsonNull
        }
        return readDetail(server, detail, episodes) ?: throw ServerError.BadAnswer()
    }

    fun readDetail(server: Server, detail: JsonElement, episodes: JsonElement): MetaDetail? {
        val id = SJ.text(detail, "Id") ?: return null
        val name = SJ.text(detail, "Name") ?: return null
        val series = SJ.text(detail, "Type") == "Series"
        val (imdb, tmdb) = providerIds(detail)
        fun people(role: String) = SJ.list(detail, "People").filter { SJ.text(it, "Type") == role }.mapNotNull { SJ.text(it, "Name") }
        val videos = SJ.list(episodes, "Items").mapNotNull { episode ->
            val episodeId = SJ.text(episode, "Id") ?: return@mapNotNull null
            Video(
                id = Servers.titleId(server.id, episodeId), title = SJ.text(episode, "Name"),
                season = SJ.number(episode, "ParentIndexNumber")?.let { SJ.whole(it) },
                episode = SJ.number(episode, "IndexNumber")?.let { SJ.whole(it) },
                released = SJ.text(episode, "PremiereDate"),
                // The episode's own still, if the server has one.
                thumbnail = remoteImage(episode, "Primary")
                    ?: posterTag(episode)?.let { image(server, episodeId, "Primary", 360, it) },
            )
        }
        val backdropTag = (SJ.list(detail, "BackdropImageTags").firstOrNull() as? JsonPrimitive)?.takeIf { it.isString }?.content
        return MetaDetail(
            id = Servers.titleId(server.id, id), kind = if (series) "series" else "movie", name = name,
            videos = videos,
            poster = remoteImage(detail, "Primary") ?: image(server, id, "Primary", 450, posterTag(detail)),
            // Asked for even when the item lists no backdrop: a server that
            // gathers its titles from elsewhere may still have one to give.
            background = remoteImage(detail, "Backdrop") ?: image(server, id, "Backdrop", 900, backdropTag),
            description = SJ.text(detail, "Overview"),
            releaseInfo = SJ.text(detail, "ProductionYear"),
            released = SJ.text(detail, "PremiereDate"),
            runtime = SJ.number(detail, "RunTimeTicks")?.let { Math.round(it / TICKS_PER_SECOND / 60).toDouble() }
                ?.takeIf { it > 0 && it.isFinite() }?.let { "${it.toLong()} min" },
            rating = SJ.number(detail, "CommunityRating")?.let { SJ.rating(it) },
            genres = SJ.list(detail, "Genres").mapNotNull { (it as? JsonPrimitive)?.takeIf { p -> p.isString }?.content },
            cast = people("Actor"), director = people("Director"),
            imdbId = imdb, tmdbId = tmdb,
        )
    }

    suspend fun report(client: ServerClient, server: Server, item: String, position: Double, finished: Boolean) {
        val headers = headers(client.device, server.kind, server.token)
        client.call(
            "${server.address}/Sessions/Playing/Stopped", headers,
            buildJsonObject { put("ItemId", item); put("PositionTicks", (maxOf(position, 0.0) * TICKS_PER_SECOND).toLong()) },
        )
        if (finished) client.call("${server.address}/Users/${server.userId}/PlayedItems/${q(item)}", headers, buildJsonObject { })
    }

    /** Trellis tells its owner which add-ons it uses, at `/api/config`. Any
     *  other server has no such page, which reads as no add-ons. */
    suspend fun sharedAddons(client: ServerClient, server: Server): List<SharedAddon> = try {
        readSharedAddons(get(client, server, "/api/config"))
    } catch (_: ServerError.Status) {
        emptyList()
    } catch (_: ServerError.BadAnswer) {
        emptyList()
    }

    fun readSharedAddons(config: JsonElement): List<SharedAddon> = SJ.list(config, "addons").mapNotNull { addon ->
        if (SJ.bool(addon, "disabled") == true) return@mapNotNull null
        val url = SJ.text(addon, "manifestUrl")?.takeIf { it.startsWith("http://") || it.startsWith("https://") }
            ?: return@mapNotNull null
        val name = SJ.text(addon, "nameOverride") ?: SJ.text(addon, "name") ?: return@mapNotNull null
        SharedAddon(name, url)
    }

    // Streams

    /** Sources handed on at a time while a long list is still arriving. */
    const val BATCH = 10
    /** The most sources read from one answer. */
    const val MOST_SOURCES = 3000
    /** How long a list of sources may take in all, and between two parts (seconds). */
    const val LIST_WAIT = 900L
    const val PART_WAIT = 45L
    /** How often an empty list of sources is asked for again, and the wait before each. */
    const val EMPTY_RETRIES = 2
    var emptyWaitMillis = 5_000L

    /** The playback information request: shaped like the server's own apps',
     *  with a profile that says this player takes any file as it is. */
    fun playbackBody(server: Server): JsonObject = buildJsonObject {
        put("UserId", server.userId)
        put("StartTimeTicks", 0)
        put("IsPlayback", false)
        put("AutoOpenLiveStream", false)
        put("MaxStreamingBitrate", 1_000_000_000)
        put("EnableDirectPlay", true)
        put("EnableDirectStream", true)
        put("EnableTranscoding", false)
        putJsonObject("DeviceProfile") {
            put("Name", "Haven")
            put("MaxStreamingBitrate", 1_000_000_000)
            put("MaxStaticBitrate", 1_000_000_000)
            putJsonArray("DirectPlayProfiles") {
                addJsonObject { put("Type", "Video") }
                addJsonObject { put("Type", "Audio") }
            }
            putJsonArray("TranscodingProfiles") {}
            putJsonArray("ContainerProfiles") {}
            putJsonArray("CodecProfiles") {}
            putJsonArray("SubtitleProfiles") {}
        }
    }

    /** Every way the server offers to play the film or episode, handed to
     *  `found` a few at a time as the answer arrives. `found` answers whether
     *  more are wanted; on no, the rest is not read. The token goes in the
     *  stream addresses because the player opens them. */
    suspend fun streamsLive(client: ServerClient, server: Server, item: String, found: (List<MediaStream>) -> Boolean) {
        val address = "${server.address}/Items/${q(item)}/PlaybackInfo?UserId=${q(server.userId)}" +
            "&StartTimeTicks=0&IsPlayback=false&AutoOpenLiveStream=false&MaxStreamingBitrate=1000000000"
        var listed = 0
        val seen = mutableSetOf<String>()
        var asked: Exception? = null
        // A server that gathers streams answers with what it has so far, which
        // can be nothing; asked again a little later it has more.
        for (attempt in 0..EMPTY_RETRIES) {
            if (attempt > 0) delay(emptyWaitMillis)
            asked = null
            try {
                listed = readPlaybackInfo(client, server, address, item, listed, seen, found)
            } catch (e: kotlinx.coroutines.CancellationException) {
                throw e
            } catch (e: Exception) {
                asked = e
            }
            Log.note("${server.name} sources: playback information " + (asked?.let { "lists $listed, then failed (${it.message})" } ?: "lists $listed"))
            if (listed > 0 || asked != null) break
        }
        if (listed > 0) return
        // Older servers, or one that listed nothing: the item itself may list its files.
        val detail = try {
            get(client, server, "/Users/${server.userId}/Items/${q(item)}?Fields=MediaSources,MediaStreams")
        } catch (e: Exception) {
            // Both failed: say why the item could not be read.
            if (asked != null) throw e
            found(readStreams(server, item, JsonObject(emptyMap())))
            return
        }
        Log.note("${server.name} sources: the item ${describeSources(detail)}")
        // An item that says outright it has no sources has none.
        if ((detail as? JsonObject)?.containsKey("MediaSources") == true && SJ.list(detail, "MediaSources").isEmpty()) return
        found(readStreams(server, item, detail))
    }

    /** Reads one playback-information answer as it arrives; says how many
     *  sources were listed in all. */
    private suspend fun readPlaybackInfo(
        client: ServerClient, server: Server, address: String, item: String,
        listedBefore: Int, seen: MutableSet<String>, found: (List<MediaStream>) -> Boolean,
    ): Int {
        var listed = listedBefore
        val response = try {
            client.http.open(
                address, method = "POST",
                headers = listOf("Accept" to "application/json") + headers(client.device, server.kind, server.token),
                body = playbackBody(server).toString().toByteArray(), readTimeoutSeconds = PART_WAIT,
            )
        } catch (e: HttpError) {
            throw ServerError.Request(e.message?.removePrefix("could not reach the server: ") ?: "")
        }
        response.use {
            when (response.code) {
                in 200..299 -> Unit
                401, 403 -> throw ServerError.SignInRefused()
                else -> throw ServerError.Status(response.code)
            }
            val deadline = System.currentTimeMillis() + LIST_WAIT * 1000
            val scanner = SourceScanner()
            val waiting = mutableListOf<JsonElement>()
            val stream = response.body?.byteStream() ?: return listed
            val buffer = ByteArray(16 * 1024)
            try {
                reading@ while (true) {
                    val n = stream.read(buffer)
                    if (n < 0) break
                    if (System.currentTimeMillis() > deadline) throw ServerError.Request("the server took too long to answer")
                    for (i in 0 until n) {
                        val source = scanner.push(buffer[i])
                        if (source == null) {
                            if (scanner.done) break@reading
                            continue
                        }
                        val id = SJ.text(source, "Id")
                        if (id != null && !seen.add(id)) continue
                        listed += 1
                        waiting += source
                        if (waiting.size >= BATCH) {
                            val wanted = found(readSources(server, item, waiting.toList(), several = listed > 1))
                            waiting.clear()
                            if (!wanted) break@reading
                        }
                        if (scanner.done || listed >= MOST_SOURCES) break@reading
                    }
                }
            } catch (_: SocketTimeoutException) {
                // Nothing more for a while: what came so far is the list.
            } catch (e: java.io.IOException) {
                if (waiting.isNotEmpty()) found(readSources(server, item, waiting.toList(), several = listed > 1))
                throw ServerError.Request(e.message ?: "the connection was lost")
            }
            if (waiting.isNotEmpty()) found(readSources(server, item, waiting.toList(), several = listed > 1))
        }
        return listed
    }

    /** What an answer's `MediaSources` look like, for the log: how many, and
     *  for each which fields it has. No ids, names or addresses. */
    fun describeSources(answer: JsonElement): String {
        val sources = SJ.list(answer, "MediaSources")
        val each = sources.take(5).map { source ->
            fun has(key: String) = if (SJ.text(source, key) == null) "no" else "yes"
            val path = SJ.text(source, "Path")
            val webPath = path?.let { it.startsWith("http://") || it.startsWith("https://") } ?: false
            "[protocol ${SJ.text(source, "Protocol") ?: "none"} id ${has("Id")} name ${has("Name")} " +
                "path ${if (webPath) "web" else has("Path")} url ${has("DirectStreamUrl")} " +
                "streams ${SJ.list(source, "MediaStreams").size}]"
        }
        val keys = (answer as? JsonObject)?.keys?.filter { it.contains("Source") || it.contains("Error") }?.sorted() ?: emptyList()
        return "lists ${sources.size} ${each.joinToString(" ")} (fields: ${keys.joinToString(", ")})"
    }

    fun fileAddress(server: Server, item: String, version: String?): String {
        var address = "${server.address}/Videos/${q(item)}/stream?static=true"
        if (version != null) address += "&MediaSourceId=${q(version)}"
        return address + "&api_key=${q(server.token)}"
    }

    /** One stream per entry of `MediaSources`; the file itself when the server lists none. */
    fun readStreams(server: Server, item: String, answer: JsonElement): List<MediaStream> {
        val versions = SJ.list(answer, "MediaSources")
        val streams = readSources(server, item, versions, several = versions.size > 1)
        if (streams.isNotEmpty()) return streams
        return listOf(MediaStream(name = "Direct", description = "The file as stored on ${server.name}", url = fileAddress(server, item, null)))
    }

    /** One stream per source; `several` says the film has more than one. */
    fun readSources(server: Server, item: String, versions: List<JsonElement>, several: Boolean): List<MediaStream> =
        versions.mapNotNull { version ->
            val id = SJ.text(version, "Id")
            fun track(kind: String) = SJ.list(version, "MediaStreams").firstOrNull { SJ.text(it, "Type") == kind }
            val video = track("Video")
            fun codec(stream: JsonElement?) = SJ.text(stream, "Codec")?.uppercase()
            val quality = Servers.quality(SJ.number(video, "Width"), SJ.number(video, "Height"))
            // A source that is itself a web address plays from there; a file plays from the server.
            val remote = SJ.text(version, "Path")?.let { path ->
                val http = SJ.text(version, "Protocol")?.equals("http", ignoreCase = true) == true
                if (http && (path.startsWith("http://") || path.startsWith("https://"))) path else null
            }
            // Or the server names the address to play it from itself.
            val given = SJ.text(version, "DirectStreamUrl")?.let { url ->
                if (url.startsWith("http://") || url.startsWith("https://")) {
                    url
                } else {
                    val join = if (url.contains("?")) "&" else "?"
                    val slash = if (url.startsWith("/")) "" else "/"
                    "${server.address}$slash$url${join}api_key=${q(server.token)}"
                }
            }
            val url = remote ?: given ?: id?.let { fileAddress(server, item, it) } ?: return@mapNotNull null
            val facts = mutableListOf<String>()
            // The server's name for the source tells several apart. When the
            // server says nothing of the picture (a gathered stream), the
            // name's first line heads the row and the rest describes it.
            var heading = quality
            SJ.text(version, "Name")?.let { name ->
                if (quality != null) {
                    if (several) facts += name
                } else {
                    val lines = name.lines().map { it.trim() }.filter { it.isNotEmpty() }
                    heading = lines.firstOrNull()
                    facts += lines.drop(1)
                }
            }
            // Containers can come as a list ("mov,mp4,m4a"): the first will do.
            SJ.text(version, "Container")?.split(",")?.firstOrNull()?.let { facts += it.uppercase() }
            codec(video)?.let { facts += it }
            codec(track("Audio"))?.let { facts += it }
            SJ.number(version, "Size")?.let { Servers.size(it) }?.let { facts += it }
            MediaStream(
                name = heading ?: "Direct",
                description = if (facts.isEmpty()) "The file as stored on ${server.name}" else facts.joinToString(" · "),
                url = url,
                // A server that gathers streams names where each came from.
                provider = SJ.text(SJ.obj(SJ.obj(version, "Remux"), "ProviderInfo"), "source"),
            )
        }
}
