package net.haven.app.livetv

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.Serializable
import net.haven.app.core.Http
import net.haven.app.core.Log
import net.haven.app.core.Problem
import net.haven.app.core.Store
import net.haven.app.core.SwitchedOff
import net.haven.app.core.TvSource
import net.haven.app.player.PlayableAddress

/** Live TV and Radio: the user's sources (M3U playlists and Xtream Codes
 *  accounts), the channels in them, the categories down the side of the
 *  page, and the user's own Favourites groups. What each screen shows is
 *  worked out here; the screens (K15, K17) only display it. */
object LiveTV {
    /** Playlists can be long, but not endless. */
    const val PLAYLIST_LIMIT = 32 * 1024 * 1024
    /** How often Xtream Codes accounts are asked whether they still answer, and how long one may take (ms). */
    const val CHECK_EVERY_MILLIS = 60_000L
    const val CHECK_WAIT_MILLIS = 8_000L

    // Favourites (kv `tv_favourites`)

    const val FAVOURITES_KEY = "tv_favourites"

    /** One of the user's own groups of channels. A channel is known by the address it plays from. */
    @Serializable
    data class FavouriteGroup(val name: String = "", val channels: List<String> = emptyList())

    fun favourites(store: Store): List<FavouriteGroup> = runCatching { store.get<List<FavouriteGroup>>(FAVOURITES_KEY) }.getOrNull() ?: emptyList()

    fun save(favourites: List<FavouriteGroup>, store: Store) {
        runCatching { store.put(FAVOURITES_KEY, favourites) }.onFailure { Log.note("could not save the favourites: ${it.message}") }
    }

    /** Puts a channel into a group, or takes it out. */
    fun toggle(address: String, group: String, favourites: List<FavouriteGroup>): List<FavouriteGroup> =
        favourites.map { g ->
            if (g.name != group) g
            else if (address in g.channels) g.copy(channels = g.channels - address)
            else g.copy(channels = g.channels + address)
        }

    /** Makes a new group (if there is none by that name); the channel being
     *  chosen for, if any, goes in. */
    fun newGroup(name: String, adding: String?, favourites: List<FavouriteGroup>): List<FavouriteGroup> {
        val clean = name.trim()
        if (clean.isEmpty()) return favourites
        var list = if (favourites.any { it.name == clean }) favourites else favourites + FavouriteGroup(clean)
        if (adding != null) list = list.map { if (it.name == clean && adding !in it.channels) it.copy(channels = it.channels + adding) else it }
        return list
    }

    /** Deletes a group; its channels stay in the playlist. */
    fun deleteGroup(name: String, favourites: List<FavouriteGroup>) = favourites.filter { it.name != name }

    // Channels

    /** Keeps the channels whose address the player may open. */
    fun playable(channels: List<Channel>) = channels.filter { PlayableAddress.stream(it.address) != null }

    /** Every channel of the switched-on sources, television and radio apart. */
    fun boards(store: Store): Pair<List<Channel>, List<Channel>> {
        val off = runCatching { SwitchedOff.load(store).tvSources }.getOrDefault(emptyList())
        val tv = mutableListOf<Channel>()
        val radio = mutableListOf<Channel>()
        for (source in runCatching { store.tvSources() }.getOrDefault(emptyList())) {
            if (source.url in off) continue
            for (channel in playable(M3U.parse(source.playlist))) if (channel.radio) radio += channel else tv += channel
        }
        return tv to radio
    }

    /** How a category is named: every channel (""), one of the playlist's
     *  groups ("g:<name>") or a Favourites group ("f:<name>"). */
    const val ALL = ""
    const val GROUP_MARK = "g:"
    const val FAVOURITE_MARK = "f:"

    data class CategoryRow(val key: String, val label: String, val count: Int, val favourite: Boolean)

    private fun words(text: String) = text.lowercase().split(Regex("\\s+")).filter { it.isNotEmpty() }

    /** The list down the side: the Favourites groups first (radio lists only
     *  those holding stations), then every channel, then the playlists' own
     *  groups that match the category filter. */
    fun categories(channels: List<Channel>, radio: Boolean, favourites: List<FavouriteGroup>, words: String): List<CategoryRow> {
        val wanted = words(words)
        val rows = mutableListOf<CategoryRow>()
        // Favourites always come first, whatever the filter says.
        for (group in favourites) {
            val count = channels.count { it.address in group.channels }
            if (!radio || count > 0) rows += CategoryRow(FAVOURITE_MARK + group.name, group.name, count, true)
        }
        rows += CategoryRow(ALL, if (radio) "All stations" else "All channels", channels.size, false)
        val counts = channels.mapNotNull { it.group }.groupingBy { it }.eachCount()
        for (name in M3U.groups(channels)) {
            if (wanted.all { name.lowercase().contains(it) }) rows += CategoryRow(GROUP_MARK + name, name, counts[name] ?: 0, false)
        }
        return rows
    }

    /** Whether a chosen category still exists (else the page goes back to all). */
    fun categoryExists(key: String, channels: List<Channel>, favourites: List<FavouriteGroup>): Boolean = when {
        key == ALL -> true
        key.startsWith(FAVOURITE_MARK) -> favourites.any { it.name == key.removePrefix(FAVOURITE_MARK) }
        key.startsWith(GROUP_MARK) -> channels.any { it.group == key.removePrefix(GROUP_MARK) }
        else -> false
    }

    data class ChannelRow(
        /** Its place among all of the board's channels (not the shown ones). */
        val index: Int,
        val name: String,
        val group: String,
        val starred: Boolean,
    )

    /** The channels in the chosen category with every word in their name. */
    fun channelRows(channels: List<Channel>, category: String, words: String, favourites: List<FavouriteGroup>): List<ChannelRow> {
        val wanted = words(words)
        fun inCategory(channel: Channel): Boolean = when {
            category == ALL -> true
            category.startsWith(FAVOURITE_MARK) -> favourites.any { it.name == category.removePrefix(FAVOURITE_MARK) && channel.address in it.channels }
            category.startsWith(GROUP_MARK) -> channel.group == category.removePrefix(GROUP_MARK)
            else -> false
        }
        return channels.withIndex().mapNotNull { (index, channel) ->
            val name = channel.name.lowercase()
            if (!inCategory(channel) || !wanted.all { name.contains(it) }) return@mapNotNull null
            ChannelRow(index, channel.name, channel.group ?: "", favourites.any { channel.address in it.channels })
        }
    }

    // Sources (Settings › Live TV)

    /** One source as Settings lists it. */
    data class SourceRow(
        val url: String,
        val name: String,
        /** Playable channels and stations in it. */
        val channels: Int,
        /** The playlist's address, or "Xtream Codes account of <user>" (an
         *  account's address holds its password and is never shown). */
        val detail: String,
        val enabled: Boolean,
        val isAccount: Boolean,
    )

    fun sourceRows(store: Store): List<SourceRow> {
        val off = runCatching { SwitchedOff.load(store).tvSources }.getOrDefault(emptyList())
        return runCatching { store.tvSources() }.getOrDefault(emptyList()).map { source ->
            val account = XtreamAccount.fromSourceUrl(source.url)
            SourceRow(
                source.url, source.name, playable(M3U.parse(source.playlist)).size,
                account?.let { "Xtream Codes account of ${it.username}" } ?: source.url,
                source.url !in off, account != null,
            )
        }
    }

    /** The Xtream Codes accounts that are switched on. */
    fun accounts(store: Store): List<XtreamAccount> {
        val off = runCatching { SwitchedOff.load(store).tvSources }.getOrDefault(emptyList())
        return runCatching { store.tvSources() }.getOrDefault(emptyList()).filter { it.url !in off }.mapNotNull { XtreamAccount.fromSourceUrl(it.url) }
    }

    /** A message for Settings › Live TV. */
    data class Status(
        val text: String,
        val isError: Boolean,
        /** It belongs under the Xtream Codes fields (not the playlist's). */
        val account: Boolean,
    )

    const val FETCHING_PLAYLIST = "Fetching the channel list…"
    const val SIGNING_IN = "Signing in and fetching the channel list…"
    const val ACCOUNT_NEEDS = "Could not add the account: give the server's web address, your user name and your password."

    /** The playlist address to add, or why it cannot be. */
    fun playlistAddress(typed: String): Pair<String?, Status?> = try {
        Http.webUrl(typed).toString() to null
    } catch (e: Exception) {
        null to Status("Could not add the source: ${e.message}.", true, false)
    }

    /** Fetches a source's channels as playlist text: a playlist's own text, or
     *  an account's live channels turned into one. Errors never hold the
     *  address (an account's holds its password). */
    suspend fun fetch(url: String, http: Http = Http.shared): Result<String> {
        XtreamAccount.fromSourceUrl(url)?.let { return fetch(it, http) }
        return try {
            Result.success(http.getBytes(url, PLAYLIST_LIMIT).toString(Charsets.UTF_8))
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Result.failure(Problem(e.message ?: "network error"))
        }
    }

    /** Signs in to an Xtream Codes account and fetches its live channels. */
    suspend fun fetch(account: XtreamAccount, http: Http = Http.shared): Result<String> = try {
        val info = Xtream.signIn(http.getBytes(account.api(), PLAYLIST_LIMIT).toString(Charsets.UTF_8))
        // The groups' names are worth having but not worth failing for.
        val categories = runCatching { http.getBytes(account.api("get_live_categories"), PLAYLIST_LIMIT).toString(Charsets.UTF_8) }.getOrDefault("")
        val streams = http.getBytes(account.api("get_live_streams"), PLAYLIST_LIMIT).toString(Charsets.UTF_8)
        Result.success(M3U.write(Xtream.channels(account, info, categories, streams)))
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        Result.failure(Problem(e.message ?: "network error"))
    }

    /** What a fetch came to. */
    data class Fetched(
        /** What to say (only when the user just added the source). */
        val status: Status? = null,
        /** An account's state now: true online, false offline, null unchanged. */
        val online: Boolean? = null,
        /** The source was saved: the pages are to be rebuilt. */
        val saved: Boolean = false,
    )

    /** Keeps a source's channels once they arrive. `adding` is true when the
     *  user just asked for it (failures are then said); false for the quiet
     *  refresh at start. An empty or unreadable list keeps what was there; a
     *  source removed while its refresh was on the way stays removed. */
    fun fetched(url: String, account: XtreamAccount?, adding: Boolean, result: Result<String>, store: Store): Fetched {
        val isAccount = account != null
        fun say(text: String, error: Boolean) = Status(text, error, isAccount)
        val known = runCatching { store.tvSources() }.getOrDefault(emptyList()).any { it.url == url }
        // An account that answered is online; one that did not is offline.
        val online = if (isAccount && (known || result.isSuccess)) result.isSuccess else null
        val playlist = result.getOrElse { problem ->
            if (adding) {
                return Fetched(say("Could not add the ${if (isAccount) "account" else "source"}: ${problem.message}.", true), online)
            }
            Log.note("could not refresh ${M3U.sourceName(Log.plainAddress(url))}: ${problem.message}")
            return Fetched(null, online)
        }
        val channels = playable(M3U.parse(playlist))
        if (channels.isEmpty()) {
            val status = if (adding) say(if (isAccount) "The account has no live channels." else "No channels were found at that address.", true) else null
            return Fetched(status, online)
        }
        if (!adding && !known) return Fetched(null, online)
        try {
            store.saveTvSource(TvSource(url, account?.label ?: M3U.sourceName(url), playlist))
        } catch (e: Exception) {
            return Fetched(say("Could not save the source: ${e.message}", true), online)
        }
        if (!adding) return Fetched(null, online, saved = true)
        val stations = channels.count { it.radio }
        fun count(n: Int, one: String, many: String) = if (n == 1) "1 $one" else "$n $many"
        val found = listOfNotNull(
            count(channels.size - stations, "channel", "channels"),
            if (stations > 0) count(stations, "radio station", "radio stations") else null,
        )
        return Fetched(say("Added ${found.joinToString(" and ")}. They are on the Live TV tab.", false), online, saved = true)
    }

    /** Switches a source on or off. Off, it stays in the list but its channels
     *  are not shown and it is not fetched. */
    fun setEnabled(url: String, on: Boolean, store: Store): Status? = try {
        val off = SwitchedOff.load(store)
        off.copy(tvSources = SwitchedOff.switched(off.tvSources, url, on)).save(store)
        null
    } catch (e: Exception) {
        Status("Could not save that: ${e.message}", true, true)
    }

    fun remove(url: String, store: Store): Status {
        try {
            store.removeTvSource(url)
        } catch (e: Exception) {
            return Status("Could not remove the source: ${e.message}", true, true)
        }
        setEnabled(url, on = true, store = store)
        return Status("Source removed.", false, true)
    }

    /** Whether an account's server answers and still accepts the sign-in. */
    suspend fun check(account: XtreamAccount, http: Http = Http.shared): Boolean {
        val data = withTimeoutOrNull(CHECK_WAIT_MILLIS) { runCatching { http.getBytes(account.api(), PLAYLIST_LIMIT) }.getOrNull() } ?: return false
        return runCatching { Xtream.signIn(data.toString(Charsets.UTF_8)) }.isSuccess
    }
}
