package net.haven.app.livetv

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import net.haven.app.core.Http
import net.haven.app.core.HttpError
import net.haven.app.core.Problem
import net.haven.app.servers.Wanted
import net.haven.app.titles.MediaStream

/** Films and series from the user's Xtream Codes accounts, offered as places
 *  to watch a title beside the media servers. An account's lists are fetched
 *  the first time they are needed in a run and kept in memory: they can be
 *  tens of thousands of titles long. */
class XtreamOnDemand(private val http: Http = Http.shared) {
    companion object {
        /** An account's whole list of films can be large. */
        const val LIST_LIMIT = 128 * 1024 * 1024
        const val LIST_WAIT_SECONDS = 120L
        const val ANSWER_LIMIT = 8 * 1024 * 1024
        /** A provider may list a series several times; this many are looked into. */
        const val SERIES_TRIED = 3

        /** An error in words that never holds the address (it holds the account's password). */
        private fun words(error: Throwable): String = when (error) {
            is HttpError.Status -> "the server answered with error ${error.code}"
            is HttpError.TooLarge -> "the list was too long"
            is HttpError.Request -> if (error.message?.contains("too long") == true) "the server took too long" else "the server could not be reached"
            is HttpError.InvalidUrl, is HttpError.UnsupportedScheme -> "the account's address is not usable"
            is XtreamError -> error.message ?: "the server could not be reached"
            else -> "the server could not be reached"
        }

        /** Whether the accounts are worth asking about a title at all. */
        fun worthAsking(wanted: Wanted): Boolean = wanted.name.isNotBlank() && !(wanted.series && wanted.episode == null)

        /** The name a stream list shows for an account. */
        fun sourceLabel(account: XtreamAccount) = "Xtream · ${account.label}"
    }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val lock = Mutex()
    /** Lists fetched (or being fetched) this run: by account and films/series. */
    private val lists = mutableMapOf<String, Deferred<List<OnDemand>>>()

    /** The account's films or series, from memory or the network. Two titles
     *  opened at once fetch a list once. */
    private suspend fun listed(account: XtreamAccount, series: Boolean): List<OnDemand> {
        val key = "${if (series) "s" else "f"}|${account.sourceUrl}"
        val task = lock.withLock {
            lists.getOrPut(key) {
                scope.async {
                    val text = http.getBytes(
                        account.api(if (series) "get_series" else "get_vod_streams"), LIST_LIMIT, timeoutSeconds = LIST_WAIT_SECONDS,
                    ).toString(Charsets.UTF_8)
                    if (series) Xtream.series(text) else Xtream.films(text)
                }
            }
        }
        return try {
            task.await()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            // A failed list is asked for again next time.
            lock.withLock { if (lists[key] === task) lists.remove(key) }
            throw e
        }
    }

    /** The ways to play the wanted film or episode from one account. */
    suspend fun streams(account: XtreamAccount, wanted: Wanted): Result<List<MediaStream>> {
        val found = try {
            Xtream.matches(listed(account, wanted.series), wanted.name, wanted.year, wanted.tmdb)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            return Result.failure(Problem(words(e)))
        }
        fun file(ext: String) = if (ext.isEmpty()) "File" else ext.uppercase()
        if (!wanted.series) {
            return Result.success(found.map { MediaStream(name = it.name, description = file(it.ext), url = account.film(it.id, it.ext)) })
        }
        val (season, episode) = wanted.episode ?: return Result.success(emptyList())
        val streams = mutableListOf<MediaStream>()
        for (show in found.take(SERIES_TRIED)) {
            val info = runCatching {
                http.getBytes("${account.api("get_series_info")}&series_id=${show.id}", ANSWER_LIMIT, timeoutSeconds = 30)
            }.getOrNull() ?: continue
            val (id, ext) = Xtream.episode(info.toString(Charsets.UTF_8), season, episode) ?: continue
            streams += MediaStream(name = "${show.name}  S$season E$episode", description = file(ext), url = account.episode(id, ext))
        }
        return Result.success(streams)
    }
}
