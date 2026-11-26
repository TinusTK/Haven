package net.haven.app.servers

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import net.haven.app.core.Http
import net.haven.app.core.HttpAnswer
import net.haven.app.core.HttpError
import net.haven.app.titles.MediaStream
import net.haven.app.titles.MetaDetail
import net.haven.app.titles.MetaPreview

/** Talks to media servers. */
class ServerClient(
    /** Identifies this copy of Haven to servers (`DeviceId`). */
    val device: String,
    val http: Http = Http.shared,
    /** An Android TV: Plex sign-in shows a short code for plex.tv/link. */
    val tv: Boolean = false,
) {
    /** Sends a request and reads a JSON answer of limited size. A `body` makes
     *  it a POST. An empty answer reads as JsonNull. */
    suspend fun call(address: String, headers: List<Pair<String, String>>, body: JsonObject? = null): JsonElement {
        runCatching { Http.webUrl(address) }.getOrElse { throw ServerError.InvalidAddress(address) }
        val answer: HttpAnswer = try {
            http.send(
                address, method = if (body == null) "GET" else "POST",
                headers = listOf("Accept" to "application/json") + headers,
                body = body?.toString()?.toByteArray(), limit = Servers.ANSWER_LIMIT,
            )
        } catch (e: HttpError) {
            throw when (e) {
                is HttpError.TooLarge -> ServerError.TooLarge()
                is HttpError.Request -> ServerError.Request(e.message?.removePrefix("could not reach the server: ") ?: "")
                is HttpError.Status -> ServerError.Status(e.code)
                is HttpError.InvalidUrl, is HttpError.UnsupportedScheme -> ServerError.InvalidAddress(address)
            }
        }
        when (answer.status) {
            in 200..299 -> Unit
            401, 403 -> throw ServerError.SignInRefused()
            else -> throw ServerError.Status(answer.status)
        }
        if (answer.body.all { it == 0x20.toByte() || it == 0x09.toByte() || it == 0x0a.toByte() || it == 0x0d.toByte() || it == 0x0c.toByte() }) {
            return JsonNull
        }
        return runCatching { Json.parseToJsonElement(answer.text) }.getOrNull() ?: throw ServerError.BadAnswer()
    }

    // Every kind of server

    /** Signs in to a Jellyfin or Emby server with a user name and password.
     *  The password is used once and not kept. */
    suspend fun signIn(kind: ServerKind, address: String, user: String, password: String): Server =
        Emby.signIn(this, kind, Servers.address(address), user, password)

    /** The server's libraries of films and series. */
    suspend fun libraries(server: Server): List<MediaLibrary> =
        if (server.kind == ServerKind.Plex) Plex.libraries(this, server) else Emby.libraries(this, server)

    /** What was added to a library most recently. */
    suspend fun latest(server: Server, library: String): List<MetaPreview> =
        if (server.kind == ServerKind.Plex) Plex.latest(this, server, library) else Emby.latest(this, server, library)

    /** A library's titles by name (the first few hundred). */
    suspend fun all(server: Server, library: String): List<MetaPreview> = page(server, library, null, 0, Servers.LIST_LENGTH)

    /** Part of a library's titles by name, narrowed to a genre if one is given
     *  (Jellyfin and Emby only). */
    suspend fun page(server: Server, library: String, genre: String?, start: Int, length: Int): List<MetaPreview> =
        if (server.kind == ServerKind.Plex) Plex.page(this, server, library, start, length)
        else Emby.page(this, server, library, genre, start, length)

    /** The genres a library or collection can be narrowed to; empty when the server does not say. */
    suspend fun genres(server: Server, library: String): List<String> =
        if (server.kind == ServerKind.Plex) emptyList() else Emby.genres(this, server, library)

    /** Whether the server answers right now. Needs no sign-in. */
    suspend fun online(server: Server): Boolean =
        if (server.kind == ServerKind.Plex) Plex.online(this, server) else Emby.online(this, server)

    /** Looks for a film or episode on the server: the item's id there, or null. */
    suspend fun find(server: Server, wanted: Wanted): String? =
        if (server.kind == ServerKind.Plex) Plex.find(this, server, wanted) else Emby.find(this, server, wanted)

    /** Titles on the server whose name matches `words`. */
    suspend fun search(server: Server, words: String): List<MetaPreview> =
        if (server.kind == ServerKind.Plex) Plex.search(this, server, words) else Emby.search(this, server, words)

    /** Everything the server knows about one film or series. */
    suspend fun meta(server: Server, item: String): MetaDetail =
        if (server.kind == ServerKind.Plex) Plex.meta(this, server, item) else Emby.meta(this, server, item)

    /** The ways to play one film or episode from the server. */
    suspend fun streams(server: Server, item: String): List<MediaStream> {
        val all = mutableListOf<MediaStream>()
        streamsLive(server, item) { all += it; true }
        return all
    }

    /** As [streams], handing the ways to `found` a few at a time while a long
     *  list is still arriving. `found` answers whether more are wanted. */
    suspend fun streamsLive(server: Server, item: String, found: (List<MediaStream>) -> Boolean) {
        if (server.kind == ServerKind.Plex) found(Plex.streams(this, server, item)) else Emby.streamsLive(this, server, item, found)
    }

    /** Tells the server how far a film or episode was watched, in seconds. */
    suspend fun report(server: Server, item: String, position: Double, duration: Double, finished: Boolean) {
        if (server.kind == ServerKind.Plex) Plex.report(this, server, item, position, duration, finished)
        else Emby.report(this, server, item, position, finished)
    }

    /** The add-ons the server itself uses, when it gathers them (Trellis) and
     *  the signed-in user owns it. Empty for every other server. */
    suspend fun sharedAddons(server: Server): List<SharedAddon> =
        if (server.kind == ServerKind.Jellyfin) Emby.sharedAddons(this, server) else emptyList()
}
