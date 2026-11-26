package net.haven.app.tracking

import kotlinx.coroutines.CancellationException
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import net.haven.app.core.Change
import net.haven.app.core.Log
import net.haven.app.core.Saved
import net.haven.app.core.Store
import net.haven.app.core.WatchProgress

/** One round of keeping Haven and the linked accounts in step: [exchange]
 *  renews tokens about to run out, sends the queued changes, and with `pull`
 *  brings in each account's Watchlist and what it says was watched.
 *  [changesSince] compares that with what the account held at its last pull
 *  ([Seen]): what was added there comes in, and what was taken off there goes
 *  here too. As the Rust build (`haven-tracking/src/sync.rs`), same kv `sync_seen`. */
object Sync {
    /** What one account held at its last pull. Keys from [listedKey] and [watchedKey]. */
    @Serializable
    data class Seen(val watchlist: Set<String> = emptySet(), val watched: Set<String> = emptySet()) {
        companion object {
            /** Every account's, by the service's name (kv `sync_seen`). */
            const val KEY = "sync_seen"
        }
    }

    /** "movie|tt1" or "series|tt1". */
    fun listedKey(title: Listed) = "${if (title.series) "series" else "movie"}|${title.imdb}"

    /** "movie|tt1|tt1" for a film, "series|tt1|tt1:2:3" for an episode. */
    fun watchedKey(entry: Watched): String = entry.episode?.let { (s, e) -> "series|${entry.imdb}|${entry.imdb}:$s:$e" }
        ?: "movie|${entry.imdb}|${entry.imdb}"

    /** The kind, title id and (for a watched key) video id in a key. */
    fun keyParts(key: String): Triple<String, String, String> {
        val parts = key.split("|", limit = 3)
        return Triple(parts.getOrElse(0) { "" }, parts.getOrElse(1) { "" }, parts.getOrElse(2) { "" })
    }

    /** Whether a change of this device's for what `key` names is still waiting to be sent: then its word stands. */
    fun waiting(queued: List<Change>, key: String): Boolean {
        val (kind, title, video) = keyParts(key)
        return queued.any { it.kind == kind && it.titleId == title && (video.isEmpty() || it.what.startsWith("watchlist") || it.videoId == video) }
    }

    /** What one account holds now; null where it could not be read. */
    data class Pulled(val service: TrackingService, val watchlist: List<Listed>? = null, val watched: List<Watched>? = null)

    /** What changed on one account since its last pull, and its new [Seen]. */
    data class AccountChanges(
        val addedListed: List<Listed> = emptyList(),
        val goneListed: List<String> = emptyList(),
        val addedWatched: List<Watched> = emptyList(),
        val goneWatched: List<String> = emptyList(),
        val seen: Seen = Seen(),
    )

    /** Compares what an account holds with what it held at its last pull
     *  (null the first time: everything is new, nothing was taken off). */
    fun changesSince(previous: Seen?, pulled: Pulled): AccountChanges {
        var changes = AccountChanges(seen = previous ?: Seen())
        pulled.watchlist?.let { listed ->
            val now = listed.map(::listedKey).toSet()
            changes = changes.copy(
                addedListed = listed.filter { previous?.watchlist?.contains(listedKey(it)) != true },
                goneListed = previous?.let { (it.watchlist - now).sorted() } ?: emptyList(),
                seen = changes.seen.copy(watchlist = now),
            )
        }
        pulled.watched?.let { watched ->
            val now = watched.map(::watchedKey).toSet()
            changes = changes.copy(
                addedWatched = watched.filter { previous?.watched?.contains(watchedKey(it)) != true },
                goneWatched = previous?.let { (it.watched - now).sorted() } ?: emptyList(),
                seen = changes.seen.copy(watched = now),
            )
        }
        return changes
    }

    /** What one round brought back. */
    data class Outcome(
        /** Queued changes every account now has (to take off the queue). */
        val sent: List<Long> = emptyList(),
        val problems: List<String> = emptyList(),
        /** Accounts whose tokens were renewed (to keep in place of the old). */
        val renewed: List<TrackedAccount> = emptyList(),
        /** What each account holds, when brought in. */
        val pulls: List<Pulled> = emptyList(),
        val pulled: Boolean = false,
    )

    /** Renews, sends `queued` to every account, and with `pull` brings in what the accounts hold. */
    suspend fun exchange(client: TrackingClient, accounts: List<TrackedAccount>, queued: List<Change>, pull: Boolean, now: Long): Outcome {
        val sent = mutableListOf<Long>()
        val problems = mutableListOf<String>()
        val renewed = mutableListOf<TrackedAccount>()
        val pulls = mutableListOf<Pulled>()
        fun problem(account: TrackedAccount, error: Throwable) {
            problems += "${account.service.label}: ${error.message}"
        }
        // A token about to run out is renewed first.
        val ready = mutableListOf<TrackedAccount>()
        for (account in accounts) {
            try {
                val fresh = client.renewed(account, now)
                if (fresh != null) {
                    renewed += fresh
                    ready += fresh
                } else {
                    ready += account
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                problem(account, e)
            }
        }
        // Each change goes to every account; it leaves the queue once all of
        // them have it (sending the same again does no harm).
        for (change in queued) {
            val marks = change.what == "watched" || change.what == "unwatched"
            val subject = Tracking.subject(change.kind, change.titleId, if (marks) change.videoId else null)
            if (subject == null) {
                sent += change.id
                continue
            }
            var all = true
            // A whole series marked watched or not: nothing the services take.
            val nothingToSend = marks && subject.series && subject.episode == null
            if (!nothingToSend) {
                for (account in ready) {
                    try {
                        when (change.what) {
                            "watched" -> client.markWatched(account, subject)
                            "unwatched" -> client.markUnwatched(account, subject)
                            "watchlist_add" -> client.setWatchlist(account, subject, true)
                            else -> client.setWatchlist(account, subject, false)
                        }
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        all = false
                        problem(account, e)
                    }
                }
            }
            if (all && ready.isNotEmpty()) sent += change.id
        }
        if (pull) {
            for (account in ready) {
                var pulled = Pulled(account.service)
                try {
                    pulled = pulled.copy(watchlist = client.watchlist(account))
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    problem(account, e)
                }
                try {
                    pulled = pulled.copy(watched = client.watched(account))
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    problem(account, e)
                }
                pulls += pulled
            }
        }
        return Outcome(sent, problems, renewed, pulls, pull)
    }

    /** Keeps renewed accounts in place of the old ones. */
    fun keepRenewed(accounts: List<TrackedAccount>, renewed: List<TrackedAccount>): List<TrackedAccount> =
        renewed.fold(accounts) { list, account -> list.filter { it.service != account.service } + account }

    /** Puts titles from an account's Watchlist on the store's, if new there.
     *  Oldest first, so the list keeps the service's order. */
    fun addListed(store: Store, listed: List<Listed>, now: Long): Int {
        var added = 0
        for ((place, title) in listed.reversed().withIndex()) {
            val kind = if (title.series) "series" else "movie"
            if (runCatching { store.inWatchlist(kind, title.imdb) }.getOrDefault(true)) continue
            if (runCatching { store.addToWatchlist(Saved(kind, title.imdb, title.title, title.year, null, now * 1000 + place)) }.isSuccess) added += 1
        }
        return added
    }

    /** Records what an account says was watched, where the store has nothing yet. */
    fun addWatched(store: Store, watched: List<Watched>): Int = watched.count { entry ->
        val (kind, videoId, detail) = entry.episode?.let { (s, e) -> Triple("series", "${entry.imdb}:$s:$e", "S$s E$e") }
            ?: Triple("movie", entry.imdb, "")
        runCatching {
            store.addWatchedElsewhere(WatchProgress(kind, entry.imdb, videoId, entry.title, detail, null, 0.0, 0.0, true, maxOf(entry.at, 0) * 1000))
        }.getOrDefault(false)
    }

    /** kv `sync_options`. The three old switches are gone: an account syncs
     *  everything or nothing (they are always written as on). */
    @Serializable
    data class Options(
        val watched: Boolean = true,
        val watchlist: Boolean = true,
        /** Also bring in what the accounts hold. */
        val pull: Boolean = true,
        /** When the queue was last sent in full, in seconds since 1970. */
        @SerialName("last_sync") val lastSync: Long = 0,
        @SerialName("last_pull") val lastPull: Long = 0,
        /** Linked accounts whose sync is switched off (by service name). */
        val off: List<String> = emptyList(),
    ) {
        fun save(store: Store) {
            runCatching { store.put(KEY, this) }.onFailure { Log.note("could not save the sync options: ${it.message}") }
        }

        companion object {
            const val KEY = "sync_options"

            fun load(store: Store): Options =
                (runCatching { store.get<Options>(KEY) }.getOrNull() ?: Options()).copy(watched = true, watchlist = true, pull = true)
        }
    }
}
