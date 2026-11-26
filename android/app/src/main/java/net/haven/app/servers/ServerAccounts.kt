package net.haven.app.servers

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.withTimeoutOrNull
import net.haven.app.core.Log
import net.haven.app.core.Store
import net.haven.app.core.SwitchedOff

/** The media servers the user signed in to: reading them, signing in
 *  (Settings › Media servers), switching on and off, removing, whether each
 *  answers, their shelves on Home, and telling a server what was watched. */
object ServerAccounts {
    /** How long a Plex code stays worth waiting for, and how often to ask (ms). */
    var plexWaitMillis = 5 * 60 * 1000L
    var plexAskEveryMillis = 2000L
    /** How often the servers are asked whether they still answer (ms). */
    const val CHECK_EVERY_MILLIS = 60_000L
    /** A server that takes longer than this to answer counts as offline (ms). */
    const val CHECK_WAIT_MILLIS = 8_000L

    /** Whether a server answers, as the list in Settings shows it. */
    enum class State { Unknown, Online, Offline }

    /** The servers on record, and those switched on (a switched-off server is
     *  listed but never asked). */
    data class Loaded(val all: List<Server> = emptyList(), val enabled: List<Server> = emptyList())

    fun load(store: Store): Loaded {
        val all = runCatching { store.servers().mapNotNull { Server.of(it) } }.getOrElse {
            Log.note("could not read the media servers: ${it.message}")
            emptyList()
        }
        val off = runCatching { SwitchedOff.load(store).servers }.getOrDefault(emptyList())
        return Loaded(all, all.filter { it.id !in off })
    }

    /** The server a `srv:…` id belongs to and the item's id on it. */
    fun serverItem(id: String, servers: List<Server>): Pair<Server, String>? {
        val (number, item) = Servers.splitId(id) ?: return null
        val server = servers.firstOrNull { it.id == number } ?: return null
        return server to item
    }

    /** Whether a server answers within [CHECK_WAIT_MILLIS]. */
    suspend fun check(server: Server, client: ServerClient): State =
        if (withTimeoutOrNull(CHECK_WAIT_MILLIS) { client.online(server) } == true) State.Online else State.Offline

    // Signing in

    /** A message for Settings › Media servers. */
    data class Status(val text: String, val isError: Boolean)

    /** Signs in to a Jellyfin or Emby server. The password is passed on once
     *  and never stored. */
    suspend fun signIn(kind: ServerKind, address: String, user: String, password: String, client: ServerClient): Result<List<Server>> =
        try {
            Result.success(listOf(client.signIn(kind, address, user, password)))
        } catch (e: CancellationException) {
            throw e
        } catch (e: ServerError) {
            Result.failure(e)
        } catch (e: Exception) {
            Result.failure(ServerError.Request(e.message ?: "network error"))
        }

    /** What signing in to Plex has come to. */
    sealed class PlexOutcome {
        data class Servers(val servers: List<Server>) : PlexOutcome()
        data class Failed(val why: String) : PlexOutcome()
    }

    /** Signs in to Plex: asks for a code, hands it to `show` (which shows it
     *  and opens the Plex page), then waits for the user to approve it there.
     *  Cancelling the coroutine stops the wait. */
    suspend fun plexSignIn(client: ServerClient, show: suspend (Plex.Pin) -> Unit): PlexOutcome {
        val pin = try {
            Plex.pin(client)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            return PlexOutcome.Failed(e.message ?: "network error")
        }
        show(pin)
        val started = System.currentTimeMillis()
        while (true) {
            delay(plexAskEveryMillis)
            if (System.currentTimeMillis() - started > plexWaitMillis) return PlexOutcome.Failed("The Plex code was not approved in time.")
            try {
                val token = Plex.token(client, pin) ?: continue
                val servers = Plex.servers(client, token)
                return if (servers.isEmpty()) PlexOutcome.Failed("This Plex account has no servers.") else PlexOutcome.Servers(servers)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                return PlexOutcome.Failed(e.message ?: "network error")
            }
        }
    }

    const val ASKING_PLEX = "Asking Plex for a code…"
    const val APPROVE_PLEX = "Approve Haven on the Plex page that just opened in your web browser."
    const val APPROVE_PLEX_TV = "On your phone or computer, go to plex.tv/link and enter the code."
    const val SIGNING_IN = "Signing in…"

    /** Keeps the servers a sign-in gave. Signing in again to one already there
     *  (same kind, address and user) keeps its number. */
    fun added(servers: List<Server>, store: Store): Status {
        val names = mutableListOf<String>()
        for (server in servers) {
            try {
                store.saveServer(server.stored)
                names += server.name
            } catch (e: Exception) {
                return Status("Could not save the server: ${e.message}", true)
            }
        }
        return Status("Signed in to ${names.joinToString(", ")}.", false)
    }

    fun failed(why: String) = Status("Could not sign in: $why.", true)

    /** Switches a server on or off. Off, Haven stays signed in to it but does
     *  not ask it for anything. */
    fun setEnabled(id: Long, on: Boolean, store: Store): Status = try {
        val off = SwitchedOff.load(store)
        off.copy(servers = SwitchedOff.switched(off.servers, id, on)).save(store)
        Status(if (on) "Server switched on." else "Server switched off.", false)
    } catch (e: Exception) {
        Status("Could not save that: ${e.message}", true)
    }

    fun remove(id: Long, store: Store): Status = try {
        store.removeServer(id)
        Status("Server removed. Haven has forgotten its sign-in.", false)
    } catch (e: Exception) {
        Status("Could not remove the server: ${e.message}", true)
    }

    // Home

    /** One shelf on Home from a server's library. */
    data class ShelfPlan(
        val title: String,
        /** The server's name, shown with the shelf. */
        val source: String,
        val server: Server,
        val library: String,
        /** What is new, or the titles by name. */
        val latest: Boolean,
    )

    /** The shelves for servers' libraries: a collection is one list, one
     *  shelf; a library also gets a shelf of what is new. */
    fun libraryPlans(listed: List<Pair<Server, List<MediaLibrary>>>): List<ShelfPlan> = buildList {
        for ((server, libraries) in listed) {
            for (library in libraries) {
                for (latest in if (library.collection) listOf(false) else listOf(true, false)) {
                    add(ShelfPlan(if (latest) "New in ${library.name}" else library.name, server.name, server, library.id, latest))
                }
            }
        }
    }

    /** The name a server's list of libraries is kept under. */
    fun librariesName(server: Server): String = "libraries|${server.id}|${server.address}"

    // Watching

    /** Tells the server a film or episode came from how far it was watched
     *  (nothing for live television, which has no length). */
    suspend fun report(server: Server, item: String, position: Double, duration: Double, finished: Boolean, client: ServerClient) {
        if (duration <= 0) return
        try {
            client.report(server, item, position, duration, finished)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Log.note("could not tell ${server.name} what was watched: ${e.message}")
        }
    }
}
