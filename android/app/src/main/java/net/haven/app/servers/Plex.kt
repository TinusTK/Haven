package net.haven.app.servers

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.buildJsonObject
import net.haven.app.core.Haven
import net.haven.app.titles.MediaStream
import net.haven.app.titles.MetaDetail
import net.haven.app.titles.MetaPreview
import net.haven.app.titles.Video

/** Plex. Signing in goes through plex.tv with a short code the user approves
 *  in a web browser (on Android TV: types in at plex.tv/link); the servers
 *  are then asked directly. */
object Plex {
    const val PLEX_TV = "https://plex.tv/api/v2"

    /** A sign-in in progress. */
    data class Pin(
        val id: String,
        /** The code plex.tv shows the user to confirm. */
        val code: String,
        /** The page to open in a web browser. */
        val link: String,
    )

    /** A server as plex.tv lists it, before an address has been tried. */
    data class Listed(
        val name: String,
        val token: String,
        /** Addresses to try, best first: local, then remote, then relayed. */
        val addresses: List<String>,
    )

    fun headers(device: String, token: String?): List<Pair<String, String>> = buildList {
        add("X-Plex-Product" to "Haven")
        add("X-Plex-Version" to Haven.version)
        add("X-Plex-Client-Identifier" to device)
        if (token != null) add("X-Plex-Token" to token)
    }

    private fun q(text: String) = Servers.query(text)

    /** Starts signing in: gives a code and the page to enter it on. */
    suspend fun pin(client: ServerClient): Pin {
        if (client.tv) {
            // A TV may have no browser: a short code to type in at plex.tv/link
            // on a phone or computer.
            val answer = client.call("$PLEX_TV/pins", headers(client.device, null), buildJsonObject { })
            val id = SJ.text(answer, "id") ?: throw ServerError.BadAnswer()
            val code = SJ.text(answer, "code") ?: throw ServerError.BadAnswer()
            return Pin(id, code, "https://plex.tv/link")
        }
        val answer = client.call("$PLEX_TV/pins?strong=true", headers(client.device, null), buildJsonObject { })
        val id = SJ.text(answer, "id") ?: throw ServerError.BadAnswer()
        val code = SJ.text(answer, "code") ?: throw ServerError.BadAnswer()
        val link = "https://app.plex.tv/auth#?clientID=${q(client.device)}&code=${q(code)}&context%5Bdevice%5D%5Bproduct%5D=Haven"
        return Pin(id, code, link)
    }

    /** Whether the user has approved the code yet: the account's token once they have. */
    suspend fun token(client: ServerClient, pin: Pin): String? =
        SJ.text(client.call("$PLEX_TV/pins/${q(pin.id)}?code=${q(pin.code)}", headers(client.device, null)), "authToken")

    fun readResources(resources: JsonElement): List<Listed> {
        val listed = mutableListOf<Listed>()
        for (resource in (resources as? JsonArray) ?: emptyList()) {
            val provides = (SJ.text(resource, "provides") ?: "").split(",")
            if (provides.none { it.trim() == "server" }) continue
            val name = SJ.text(resource, "name") ?: continue
            val token = SJ.text(resource, "accessToken") ?: continue
            // Local first, then remote, then relayed; the same rank keeps its order.
            val ranked = mutableListOf<Triple<Int, Int, String>>()
            for (connection in SJ.list(resource, "connections")) {
                val uri = SJ.text(connection, "uri")?.trimEnd('/') ?: continue
                val rank = when {
                    SJ.bool(connection, "relay") == true -> 2
                    SJ.bool(connection, "local") == true -> 0
                    else -> 1
                }
                ranked += Triple(rank, ranked.size, uri)
            }
            listed += Listed(name, token, ranked.sortedWith(compareBy({ it.first }, { it.second })).map { it.third })
        }
        return listed
    }

    /** The Plex servers the account can use, each with an address that answered from here. */
    suspend fun servers(client: ServerClient, accountToken: String): List<Server> {
        val resources = client.call("$PLEX_TV/resources?includeHttps=1&includeRelay=1", headers(client.device, accountToken))
        val listed = readResources(resources)
        if (listed.isEmpty()) return emptyList()
        val found = mutableListOf<Server>()
        for (server in listed) {
            for (address in server.addresses) {
                if (runCatching { client.call("$address/identity", headers(client.device, server.token)) }.isSuccess) {
                    found += Server(kind = ServerKind.Plex, name = server.name, address = address, token = server.token)
                    break
                }
            }
        }
        if (found.isEmpty()) throw ServerError.Unreachable()
        return found
    }

    private suspend fun get(client: ServerClient, server: Server, path: String): JsonElement =
        SJ.obj(client.call(server.address + path, headers(client.device, server.token)), "MediaContainer") ?: JsonNull

    suspend fun libraries(client: ServerClient, server: Server): List<MediaLibrary> = readLibraries(get(client, server, "/library/sections"))

    fun readLibraries(container: JsonElement): List<MediaLibrary> =
        SJ.list(container, "Directory").filter { (SJ.text(it, "type") ?: "") in listOf("movie", "show") }.mapNotNull { section ->
            val id = SJ.text(section, "key") ?: return@mapNotNull null
            val name = SJ.text(section, "title") ?: return@mapNotNull null
            MediaLibrary(id, name, collection = false)
        }

    private suspend fun listPage(client: ServerClient, server: Server, path: String, start: Int, length: Int): List<MetaPreview> {
        val join = if (path.contains("?")) "&" else "?"
        return readPreviews(server, get(client, server, "$path${join}X-Plex-Container-Start=$start&X-Plex-Container-Size=$length"))
    }

    suspend fun latest(client: ServerClient, server: Server, library: String) =
        listPage(client, server, "/library/sections/${q(library)}/recentlyAdded", 0, 30)

    suspend fun page(client: ServerClient, server: Server, library: String, start: Int, length: Int) =
        listPage(client, server, "/library/sections/${q(library)}/all?sort=titleSort", start, length)

    suspend fun search(client: ServerClient, server: Server, words: String) =
        listPage(client, server, "/search?query=${q(words)}", 0, 30)

    suspend fun online(client: ServerClient, server: Server): Boolean =
        runCatching { client.call("${server.address}/identity", headers(client.device, null)) }.isSuccess

    /** A title's ids at IMDb and TMDB: from its `Guid` list, or from the single
     *  `guid` older libraries carry. */
    fun providerIds(item: JsonElement?): Pair<String?, String?> {
        val guids = SJ.list(item, "Guid").mapNotNull { SJ.text(it, "id") } + listOfNotNull(SJ.text(item, "guid"))
        fun id(marks: List<String>): String? {
            for (guid in guids) {
                val mark = marks.firstNotNullOfOrNull { m -> guid.indexOf(m).takeIf { it >= 0 }?.let { it + m.length } } ?: continue
                val rest = guid.substring(mark)
                val id = rest.split('?', '/').first().trim()
                if (id.isNotEmpty()) return id
            }
            return null
        }
        return id(listOf("imdb://")) to id(listOf("tmdb://", "themoviedb://"))
    }

    fun readCandidates(container: JsonElement): List<Candidate> = SJ.list(container, "Metadata").mapNotNull { item ->
        val id = SJ.text(item, "ratingKey") ?: return@mapNotNull null
        val name = SJ.text(item, "title") ?: return@mapNotNull null
        val (imdb, tmdb) = providerIds(item)
        Candidate(id, name, SJ.text(item, "year"), imdb, tmdb)
    }

    fun readEpisode(leaves: JsonElement, season: Int, number: Int): String? =
        SJ.list(leaves, "Metadata").firstOrNull {
            SJ.number(it, "parentIndex") == season.toDouble() && SJ.number(it, "index") == number.toDouble()
        }?.let { SJ.text(it, "ratingKey") }

    suspend fun find(client: ServerClient, server: Server, wanted: Wanted): String? {
        val name = wanted.name.trim()
        if (name.isEmpty()) return null
        // Type 1 is films, 2 is shows.
        val path = "/library/all?type=${if (wanted.series) 2 else 1}&title=${q(name)}&includeGuids=1" +
            "&X-Plex-Container-Start=0&X-Plex-Container-Size=25"
        val found = Servers.bestMatch(wanted, readCandidates(get(client, server, path))) ?: return null
        val episode = wanted.episode ?: return if (wanted.series) null else found.id
        val leaves = get(client, server, "/library/metadata/${q(found.id)}/allLeaves")
        return readEpisode(leaves, episode.first, episode.second)
    }

    /** A picture (or file) on the server; the token goes in the address. */
    fun image(server: Server, path: String): String = "${server.address}$path?X-Plex-Token=${q(server.token)}"

    /** Films and shows as posters. A season or episode stands for its show; a show appears once. */
    fun readPreviews(server: Server, container: JsonElement): List<MetaPreview> {
        val previews = mutableListOf<MetaPreview>()
        for (item in SJ.list(container, "Metadata")) {
            val (kind, id, name, thumb) = when (SJ.text(item, "type")) {
                "movie" -> listOf("movie", SJ.text(item, "ratingKey"), SJ.text(item, "title"), SJ.text(item, "thumb"))
                "show" -> listOf("series", SJ.text(item, "ratingKey"), SJ.text(item, "title"), SJ.text(item, "thumb"))
                "season" -> listOf("series", SJ.text(item, "parentRatingKey"), SJ.text(item, "parentTitle"), SJ.text(item, "parentThumb"))
                "episode" -> listOf(
                    "series", SJ.text(item, "grandparentRatingKey"), SJ.text(item, "grandparentTitle"), SJ.text(item, "grandparentThumb"),
                )
                else -> continue
            }
            if (id == null || name == null) continue
            val full = Servers.titleId(server.id, id)
            if (previews.any { it.id == full }) continue
            previews += MetaPreview(
                id = full, kind = kind!!, name = name, poster = thumb?.let { image(server, it) },
                releaseInfo = SJ.text(item, "year"),
                rating = (SJ.number(item, "audienceRating") ?: SJ.number(item, "rating"))?.takeIf { it > 0 }?.let { SJ.rating(it) },
            )
        }
        return previews
    }

    suspend fun meta(client: ServerClient, server: Server, item: String): MetaDetail {
        val key = q(item)
        val detail = get(client, server, "/library/metadata/$key")
        val show = SJ.text(SJ.list(detail, "Metadata").firstOrNull(), "type") == "show"
        val episodes = if (show) get(client, server, "/library/metadata/$key/allLeaves") else JsonNull
        return readDetail(server, detail, episodes) ?: throw ServerError.BadAnswer()
    }

    fun readDetail(server: Server, detail: JsonElement, episodes: JsonElement): MetaDetail? {
        val item = SJ.list(detail, "Metadata").firstOrNull() ?: return null
        val id = SJ.text(item, "ratingKey") ?: return null
        val name = SJ.text(item, "title") ?: return null
        val (imdb, tmdb) = providerIds(item)
        fun tags(key: String) = SJ.list(item, key).mapNotNull { SJ.text(it, "tag") }
        val videos = SJ.list(episodes, "Metadata").mapNotNull { episode ->
            val episodeId = SJ.text(episode, "ratingKey") ?: return@mapNotNull null
            Video(
                id = Servers.titleId(server.id, episodeId), title = SJ.text(episode, "title"),
                season = SJ.number(episode, "parentIndex")?.let { SJ.whole(it) },
                episode = SJ.number(episode, "index")?.let { SJ.whole(it) },
                released = SJ.text(episode, "originallyAvailableAt"),
                thumbnail = SJ.text(episode, "thumb")?.let { image(server, it) },
            )
        }
        return MetaDetail(
            id = Servers.titleId(server.id, id), kind = if (SJ.text(item, "type") == "show") "series" else "movie", name = name,
            videos = videos,
            poster = SJ.text(item, "thumb")?.let { image(server, it) },
            background = SJ.text(item, "art")?.let { image(server, it) },
            description = SJ.text(item, "summary"),
            releaseInfo = SJ.text(item, "year"),
            released = SJ.text(item, "originallyAvailableAt"),
            // Plex counts in milliseconds.
            runtime = SJ.number(item, "duration")?.let { Math.round(it / 60_000).toDouble() }
                ?.takeIf { it > 0 && it.isFinite() }?.let { "${it.toLong()} min" },
            rating = (SJ.number(item, "rating") ?: SJ.number(item, "audienceRating"))?.let { SJ.rating(it) },
            genres = tags("Genre"), cast = tags("Role"), director = tags("Director"),
            imdbId = imdb, tmdbId = tmdb,
        )
    }

    suspend fun streams(client: ServerClient, server: Server, item: String): List<MediaStream> =
        readStreams(server, get(client, server, "/library/metadata/${q(item)}"))

    /** One stream per version of the file the server holds. */
    fun readStreams(server: Server, detail: JsonElement): List<MediaStream> {
        val item = SJ.list(detail, "Metadata").firstOrNull() ?: return emptyList()
        return SJ.list(item, "Media").mapNotNull { media ->
            val part = SJ.list(media, "Part").firstOrNull() ?: return@mapNotNull null
            val key = SJ.text(part, "key") ?: return@mapNotNull null
            val quality = SJ.text(media, "videoResolution")?.let {
                when (it) {
                    "4k" -> "4K"
                    "sd" -> "SD"
                    else -> "${it}p"
                }
            } ?: "Direct"
            val facts = listOf(SJ.text(media, "container"), SJ.text(media, "videoCodec"), SJ.text(media, "audioCodec"))
                .mapNotNull { it?.uppercase() }.toMutableList()
            SJ.number(part, "size")?.let { Servers.size(it) }?.let { facts += it }
            MediaStream(
                name = quality,
                description = if (facts.isEmpty()) "The file as stored on ${server.name}" else facts.joinToString(" · "),
                url = image(server, key),
            )
        }
    }

    suspend fun report(client: ServerClient, server: Server, item: String, position: Double, duration: Double, finished: Boolean) {
        val key = q(item)
        val path = if (finished) {
            "/:/scrobble?identifier=com.plexapp.plugins.library&key=$key"
        } else {
            "/:/timeline?ratingKey=$key&key=%2Flibrary%2Fmetadata%2F$key&state=stopped" +
                "&time=${(maxOf(position, 0.0) * 1000).toLong()}&duration=${(maxOf(duration, 0.0) * 1000).toLong()}" +
                "&identifier=com.plexapp.plugins.library"
        }
        get(client, server, path)
    }
}
