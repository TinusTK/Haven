package net.haven.app.livetv

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import net.haven.app.core.Http
import net.haven.app.core.Store
import net.haven.app.core.SwitchedOff
import net.haven.app.library.PlayRequest

/**
 * The Live TV page: television and radio, each with its categories (the
 * Favourites groups first) and the chosen category's channels, and the
 * user's Favourites groups. Sources are fetched again quietly at start; Xtream
 * Codes accounts are asked every minute whether they answer. The Rust build's
 * `livetv.rs` (the Settings part comes in K17).
 */
class LiveTVModel(private val store: Store, private val scope: CoroutineScope, private val http: Http = Http.shared) {
    /** Television or radio: its channels and the filters on them. */
    data class Board(
        val channels: List<Channel> = emptyList(),
        /** The chosen category's key; empty for every channel. */
        val category: String = LiveTV.ALL,
        /** Words the channels' names must hold. */
        val words: String = "",
        /** Words the categories' names must hold. */
        val categoryWords: String = "",
    )

    var tv by mutableStateOf(Board())
        private set
    var radio by mutableStateOf(Board())
        private set
    var favourites by mutableStateOf(LiveTV.favourites(store))
        private set
    /** 0 television, 1 radio. */
    var band by mutableIntStateOf(0)
        private set
    /** Showing the categories instead of the channels. */
    var picking by mutableStateOf(false)
    /** The channel whose Favourites groups are being chosen. */
    var choosing by mutableStateOf<Channel?>(null)
        private set
    /** Whether each Xtream Codes account answers, by its source address. */
    var online by mutableStateOf(mapOf<String, Boolean>())
        private set
    /** Plays a channel. */
    var play: (PlayRequest) -> Unit = {}
    private var checker: Job? = null

    /** Reads the Favourites again (brought in from another device). */
    fun reloadFavourites() {
        favourites = LiveTV.favourites(store)
    }

    /** Shows the stored sources' channels, then fetches each source again. */
    fun start() {
        rebuild()
        val off = runCatching { SwitchedOff.load(store).tvSources }.getOrDefault(emptyList())
        for (source in runCatching { store.tvSources() }.getOrDefault(emptyList())) if (source.url !in off) refresh(source.url)
        checker?.cancel()
        checker = scope.launch {
            while (isActive) {
                delay(LiveTV.CHECK_EVERY_MILLIS)
                checkAccounts()
            }
        }
    }

    /** Fetches one source again, quietly: an answer that cannot be read keeps
     *  what was there. */
    fun refresh(url: String) {
        scope.launch {
            val result = LiveTV.fetch(url, http)
            val fetched = LiveTV.fetched(url, XtreamAccount.fromSourceUrl(url), adding = false, result = result, store = store)
            fetched.online?.let { online = online + (url to it) }
            if (fetched.saved) rebuild()
        }
    }

    /** Asks every switched-on Xtream Codes account whether it answers. */
    suspend fun checkAccounts() {
        for (account in LiveTV.accounts(store)) online = online + (account.sourceUrl to LiveTV.check(account, http))
    }

    /** Reads every stored playlist again and refills both boards. */
    fun rebuild() {
        val (tvChannels, radioChannels) = LiveTV.boards(store)
        tv = tv.copy(channels = tvChannels)
        radio = radio.copy(channels = radioChannels)
        keepCategory()
    }

    /** A chosen category that is gone falls back to every channel. */
    private fun keepCategory() {
        if (!LiveTV.categoryExists(tv.category, tv.channels, favourites)) tv = tv.copy(category = LiveTV.ALL)
        if (!LiveTV.categoryExists(radio.category, radio.channels, favourites)) radio = radio.copy(category = LiveTV.ALL)
    }

    // What the page shows

    val isRadio get() = band == 1
    val board get() = if (isRadio) radio else tv

    val categories get() = LiveTV.categories(board.channels, isRadio, favourites, board.categoryWords)
    val channels get() = LiveTV.channelRows(board.channels, board.category, board.words, favourites)

    /** "120 in all", or "12 of 120". */
    val countText: String get() {
        val shown = channels.size
        val total = board.channels.size
        return if (shown == total) "$total in all" else "$shown of $total"
    }

    /** The chosen category's name. */
    val categoryName get() = categories.firstOrNull { it.key == board.category }?.label ?: if (isRadio) "All stations" else "All channels"

    fun chooseBand(band: Int) {
        this.band = band
        picking = false
    }

    private fun edit(change: (Board) -> Board) {
        if (isRadio) radio = change(radio) else tv = change(tv)
    }

    fun setWords(words: String) = edit { it.copy(words = words) }
    fun setCategoryWords(words: String) = edit { it.copy(categoryWords = words) }

    fun choose(key: String) {
        edit { it.copy(category = key) }
        picking = false
    }

    // Playing

    /** Plays a channel by its place among all of the board's channels. */
    fun play(index: Int) {
        val channel = board.channels.getOrNull(index) ?: return
        // A channel has no place to come back to.
        play(PlayRequest(channel.address, channel.name, channel.group ?: "", if (isRadio) "radio" else "tv", "", "", channel.logo, "", remember = false))
    }

    // Favourites

    /** Opens the chooser of Favourites groups for a channel. */
    fun chooseFavourites(index: Int) {
        choosing = board.channels.getOrNull(index) ?: return
    }

    fun closeChooser() {
        choosing = null
    }

    /** Whether the channel being chosen for is in a group. */
    fun has(group: LiveTV.FavouriteGroup) = choosing?.let { it.address in group.channels } ?: false

    fun toggle(name: String) {
        val address = choosing?.address ?: return
        save(LiveTV.toggle(address, name, favourites))
    }

    /** Makes a new group; the channel being chosen for, if any, goes in. */
    fun newGroup(name: String) = save(LiveTV.newGroup(name, choosing?.address, favourites))

    fun deleteGroup(name: String) = save(LiveTV.deleteGroup(name, favourites))

    private fun save(list: List<LiveTV.FavouriteGroup>) {
        favourites = list
        LiveTV.save(list, store)
        keepCategory()
    }
}
