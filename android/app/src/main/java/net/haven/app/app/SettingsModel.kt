package net.haven.app.app

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import net.haven.app.addons.InstalledAddons
import net.haven.app.addons.Stremio
import net.haven.app.core.AppPaths
import net.haven.app.core.Backup
import net.haven.app.core.Keys
import net.haven.app.core.Log
import net.haven.app.core.Secrets
import net.haven.app.core.Store
import net.haven.app.library.Library
import net.haven.app.livetv.LiveTV
import net.haven.app.livetv.LiveTVModel
import net.haven.app.livetv.XtreamAccount
import net.haven.app.metadata.WhereToWatch
import net.haven.app.player.PlayerRules
import net.haven.app.servers.Server
import net.haven.app.servers.ServerAccounts
import net.haven.app.servers.ServerKind
import net.haven.app.tracking.Tracker
import java.io.File

/** Settings: what each part shows and does. The rules behind each part
 *  (servers, Live TV sources, add-ons, keys, tracking, where to watch, the
 *  player, the backup) were ported in their own sections; this joins them
 *  to the page. */
class SettingsModel(
    private val store: Store,
    private val paths: AppPaths,
    private val library: Library,
    private val tracker: Tracker,
    private val live: LiveTVModel,
    private val scope: CoroutineScope,
    /** The GitHub build: the Add-ons page, installing by address. */
    val addonScreens: Boolean,
) {
    /** The parts of Settings, in the Rust build's numbering. */
    enum class Section(val label: String, val icon: String) {
        General("General", "general"),
        Accounts("Accounts", "account"),
        Servers("Media servers", "server"),
        Addons("Add-ons", "addon"),
        LiveTV("Live TV", "livetv"),
        Storage("Downloads & storage", "storage"),
        About("About", "info"),
        Sync("Sync options", "sync");

        companion object {
            /** The list, under its groups (Add-ons only in the GitHub build). */
            fun groups(addons: Boolean): List<Pair<String, List<Section>>> = listOf(
                "General" to listOf(General, Storage),
                "Connections" to if (addons) listOf(Accounts, Sync, Servers, Addons, LiveTV) else listOf(Accounts, Sync, Servers, LiveTV),
                "Haven" to listOf(About),
            )
        }
    }

    /** The part open; null shows the list. */
    var section by mutableStateOf<Section?>(null)
    /** Goes up when something saved changes, so the page reads it again. */
    var revision by mutableIntStateOf(0)
        private set

    /** Opens a web page (set by the app). */
    var openLink: (String) -> Unit = {}
    /** The player's settings (null until the app has a player). */
    var player: () -> net.haven.app.player.PlayerModel? = { null }
    /** The picture cache's line and how to clear it (set by the app). */
    var cacheText: () -> String = { "" }
    var clearPictures: () -> Unit = {}
    /** Where the backup goes, and in words (set by the app). */
    var backupFolder: () -> Pair<File, String> = { File(paths.dataDir, "backup") to "Haven's own folder" }
    /** Seconds since 1970 (a stand-in in tests). */
    var now: () -> Long = { System.currentTimeMillis() / 1000 }

    private fun changed() { revision += 1 }

    /** Shows what changed underneath (brought in from another device). */
    fun refresh() = changed()

    // General

    val countries: List<String> get() = WhereToWatch.settingsList()

    val country: Int get() = revision.let { WhereToWatch.settingsIndex(store) }

    fun setCountry(index: Int) {
        runCatching { WhereToWatch.choose(index, store) }.onFailure { Log.note("could not save the country: ${it.message}") }
        changed()
    }

    /** 0 Haven's own player, 1 another player app. */
    val otherPlayer: Int get() = revision.let { if (PlayerRules.OtherPlayer.load(store).kind == 0) 0 else 1 }

    fun setOtherPlayer(kind: Int) {
        val choice = PlayerRules.OtherPlayer.load(store).copy(kind = kind.coerceIn(0, 1))
        runCatching { store.put(PlayerRules.OtherPlayer.KEY, choice) }.onFailure { Log.note("could not save that: ${it.message}") }
        changed()
    }

    private fun preferences() = player()?.preferences ?: PlayerRules.Preferences.load(store)

    val passthrough: Boolean get() = revision.let { preferences().passthrough }
    val loud: Boolean get() = revision.let { preferences().loud }

    fun setPassthrough(on: Boolean) = setPreference({ it.setPassthrough(on) }) { it.copy(passthrough = on) }
    fun setLoud(on: Boolean) = setPreference({ it.setLoud(on) }) { it.copy(loud = on) }

    /** Through the player when there is one (it uses it at once), else saved. */
    private fun setPreference(
        live: (net.haven.app.player.PlayerModel) -> Unit,
        edit: (PlayerRules.Preferences) -> PlayerRules.Preferences,
    ) {
        val player = player()
        if (player != null) live(player) else runCatching { store.put(PlayerRules.Preferences.KEY, edit(PlayerRules.Preferences.load(store))) }
        changed()
    }

    // Accounts: your own keys

    var keys by mutableStateOf(runCatching { Keys.load(store) }.getOrDefault(Keys()))
    var keysStatus by mutableStateOf("")
        private set

    fun saveKeys() {
        try {
            keys.save(store)
        } catch (e: Exception) {
            keysStatus = "Could not save the keys: ${e.message}"
            return
        }
        val resolved = keys.resolved(runCatching { Secrets.fromFile(paths.secretsFile) }.getOrDefault(Secrets()))
        library.useKeys(resolved)
        tracker.setKeys(resolved)
        keysStatus = "Keys saved."
    }

    // Media servers

    var serverKind by mutableStateOf(ServerKind.Jellyfin)
    var serverAddress by mutableStateOf("")
    var serverUser by mutableStateOf("")
    var serverPassword by mutableStateOf("")
    var serverStatus by mutableStateOf<ServerAccounts.Status?>(null)
        private set
    var signingIn by mutableStateOf(false)
        private set
    /** The Plex code to approve, and the page to approve it on. */
    var plexCode by mutableStateOf("")
        private set
    var plexLink by mutableStateOf("")
        private set
    private var signInJob: Job? = null

    val servers: List<Server> get() = library.servers.all

    fun serverOn(server: Server) = library.servers.enabled.any { it.id == server.id }

    fun serverState(server: Server) = library.serverStates[server.id] ?: ServerAccounts.State.Unknown

    /** `tv`: Plex is approved on another device (a TV opens no web page). */
    fun signIn(tv: Boolean = false) {
        if (signingIn) return
        signingIn = true
        val client = library.serverClient
        if (serverKind == ServerKind.Plex) {
            serverStatus = ServerAccounts.Status(ServerAccounts.ASKING_PLEX, false)
            signInJob = scope.launch {
                val outcome = ServerAccounts.plexSignIn(client) { pin ->
                    plexCode = pin.code
                    plexLink = pin.link
                    serverStatus = ServerAccounts.Status(if (tv) ServerAccounts.APPROVE_PLEX_TV else ServerAccounts.APPROVE_PLEX, false)
                    if (!tv) openLink(pin.link)
                }
                when (outcome) {
                    is ServerAccounts.PlexOutcome.Servers -> added(outcome.servers)
                    is ServerAccounts.PlexOutcome.Failed -> finishSignIn(ServerAccounts.failed(outcome.why))
                }
            }
            return
        }
        serverStatus = ServerAccounts.Status(ServerAccounts.SIGNING_IN, false)
        val kind = if (serverKind == ServerKind.Emby) ServerKind.Emby else ServerKind.Jellyfin
        val address = serverAddress
        val user = serverUser
        val password = serverPassword
        signInJob = scope.launch {
            ServerAccounts.signIn(kind, address, user, password, client).fold(
                onSuccess = { added(it) },
                onFailure = { finishSignIn(ServerAccounts.failed(it.message ?: "unknown error")) },
            )
        }
    }

    fun cancelSignIn() {
        signInJob?.cancel()
        finishSignIn(null)
    }

    private fun added(found: List<Server>) {
        val status = ServerAccounts.added(found, store)
        if (!status.isError) {
            serverAddress = ""
            serverUser = ""
        }
        finishSignIn(status)
        library.loadServers()
    }

    private fun finishSignIn(status: ServerAccounts.Status?) {
        signingIn = false
        serverPassword = ""
        plexCode = ""
        plexLink = ""
        serverStatus = status
    }

    fun setServer(server: Server, on: Boolean) {
        serverStatus = ServerAccounts.setEnabled(server.id, on, store)
        library.loadServers()
    }

    fun removeServer(server: Server) {
        serverStatus = ServerAccounts.remove(server.id, store)
        library.loadServers()
    }

    /** Asks every server again whether it answers. */
    fun checkServers() = library.reloadServerShelves()

    // Live TV sources

    var playlistAddress by mutableStateOf("")
    var xtreamServer by mutableStateOf("")
    var xtreamUser by mutableStateOf("")
    var xtreamPassword by mutableStateOf("")
    var tvStatus by mutableStateOf<LiveTV.Status?>(null)
        private set
    var tvBusy by mutableStateOf(false)
        private set

    val tvSources: List<LiveTV.SourceRow> get() = revision.let { LiveTV.sourceRows(store) }

    /** An account's state: null while not known (or a playlist, or off). */
    fun tvOnline(row: LiveTV.SourceRow): Boolean? = if (row.isAccount && row.enabled) live.online[row.url] else null

    fun addPlaylist() {
        if (tvBusy) return
        val (url, problem) = LiveTV.playlistAddress(playlistAddress)
        if (url == null) {
            tvStatus = problem
            return
        }
        tvBusy = true
        tvStatus = LiveTV.Status(LiveTV.FETCHING_PLAYLIST, false, false)
        scope.launch { finishAdding(url, null, LiveTV.fetch(url)) }
    }

    fun addXtream() {
        if (tvBusy) return
        val account = XtreamAccount.of(xtreamServer, xtreamUser, xtreamPassword)
        if (account == null) {
            tvStatus = LiveTV.Status(LiveTV.ACCOUNT_NEEDS, true, true)
            return
        }
        tvBusy = true
        tvStatus = LiveTV.Status(LiveTV.SIGNING_IN, false, true)
        scope.launch { finishAdding(account.sourceUrl, account, LiveTV.fetch(account)) }
    }

    private fun finishAdding(url: String, account: XtreamAccount?, result: Result<String>) {
        tvBusy = false
        val fetched = LiveTV.fetched(url, account, adding = true, result = result, store = store)
        tvStatus = fetched.status
        if (fetched.saved) {
            if (account == null) {
                playlistAddress = ""
            } else {
                xtreamServer = ""
                xtreamUser = ""
                xtreamPassword = ""
            }
            live.rebuild()
            scope.launch { live.checkAccounts() }
        }
        changed()
    }

    fun setTVSource(url: String, on: Boolean) {
        LiveTV.setEnabled(url, on, store)?.let { tvStatus = it }
        live.rebuild()
        if (on) live.refresh(url)
        changed()
    }

    fun removeTVSource(url: String) {
        tvStatus = LiveTV.remove(url, store)
        live.rebuild()
        changed()
    }

    // Add-ons (GitHub build)

    var addonAddress by mutableStateOf("")
    var addonStatus by mutableStateOf<InstalledAddons.Outcome?>(null)
        private set
    var addonBusy by mutableStateOf(false)
        private set

    val addons: List<InstalledAddons.Row> get() = revision.let { library.addons.rows }

    fun installAddon() {
        if (!addonScreens || addonBusy) return
        val address = addonAddress
        addonBusy = true
        scope.launch {
            val outcome = InstalledAddons.install(address, store)
            addonBusy = false
            addonStatus = outcome
            if (outcome is InstalledAddons.Outcome.Installed) addonAddress = ""
            addonsChanged()
        }
    }

    fun setAddon(url: String, on: Boolean) {
        runCatching { InstalledAddons.setEnabled(url, on, store) }
        addonsChanged()
    }

    fun removeAddon(url: String) {
        runCatching { InstalledAddons.remove(url, store) }
        addonsChanged()
    }

    private fun addonsChanged() {
        library.loadSources()
        library.reloadHome()
        changed()
    }

    // Stremio account (both Android builds; the Play build follows the
    // account's whole list, as nothing is installed there by hand)

    var stremioEmailEntered by mutableStateOf("")
    var stremioPasswordEntered by mutableStateOf("")
    var stremioStatus by mutableStateOf("")
        private set
    var stremioBusy by mutableStateOf(false)
        private set

    /** The signed-in account's email; empty when signed out. */
    val stremioEmail: String get() = revision.let { Stremio.account(store)?.email ?: "" }

    fun stremioSignIn() {
        if (stremioBusy) return
        val email = stremioEmailEntered
        val password = stremioPasswordEntered
        if (email.isBlank() || password.isEmpty()) {
            stremioStatus = "Type your Stremio email address and password."
            return
        }
        stremioBusy = true
        stremioStatus = "Signing in to Stremio…"
        scope.launch {
            try {
                val account = Stremio.login(email.trim(), password)
                val listed = Stremio.listed(account)
                store.put(Stremio.ACCOUNT_KEY, account)
                stremioPasswordEntered = ""
                stremioStatus = ""
                if (Stremio.follow(listed, store, wholeList = !addonScreens)) addonsChanged() else changed()
            } catch (e: Exception) {
                if (e is kotlinx.coroutines.CancellationException) throw e
                stremioStatus = "Could not sign in: ${e.message}."
            }
            stremioBusy = false
        }
    }

    fun stremioSignOut() {
        val account = Stremio.account(store) ?: return
        runCatching { store.remove(Stremio.ACCOUNT_KEY) }
        // What came from the account goes.
        if (runCatching { Stremio.follow(emptyList(), store, wholeList = !addonScreens) }.getOrDefault(false)) addonsChanged() else changed()
        stremioStatus = "Signed out of Stremio."
        scope.launch { runCatching { Stremio.logout(account) } }
    }

    /** At start: the account's list again. Offline, what is installed stays. */
    fun stremioRefresh() {
        val account = Stremio.account(store) ?: return
        scope.launch {
            try {
                val listed = Stremio.listed(account)
                if (Stremio.follow(listed, store, wholeList = !addonScreens)) addonsChanged()
            } catch (e: Stremio.Failure.Refused) {
                stremioStatus = "Stremio asks you to sign in again."
            } catch (e: Exception) {
                if (e is kotlinx.coroutines.CancellationException) throw e
                Log.note("stremio: kept the last list: ${e.message}")
            }
        }
    }

    // Storage

    fun clearCache() {
        clearPictures()
        changed()
    }

    // Backup

    var backupStatus by mutableStateOf("")
        private set
    var backupBusy by mutableStateOf(false)
        private set
    private var lastWritten = ""
    private var backupPlace = ""
    private var backupTimer: Job? = null

    /** Backs up a little after start and then every 10 minutes, when
     *  something changed. */
    fun startBackups() {
        showBackup()
        backupTimer?.cancel()
        backupTimer = scope.launch {
            delay(Backup.AFTER_START_MILLIS)
            while (isActive) {
                backUp(quiet = true)
                delay(Backup.EVERY_MILLIS)
            }
        }
    }

    private fun showBackup(note: String = "") {
        val last = runCatching { store.get<Long>(Backup.LAST_KEY) }.getOrNull() ?: 0L
        backupStatus = when {
            note.isNotEmpty() -> note
            last == 0L -> Backup.FIRST_NOTE
            backupPlace.isEmpty() -> "Last backed up ${Backup.`when`(last)}."
            else -> "Last backed up ${Backup.`when`(last)} to $backupPlace."
        }
    }

    /** Writes the backup if anything changed since the last one (or, not
     *  `quiet`, always). */
    fun backUp(quiet: Boolean = false) {
        if (backupBusy) return
        val backup = try {
            Backup.make(store, sources = addonScreens, now = now())
        } catch (e: Exception) {
            return showBackup("Could not read what to back up: ${e.message}")
        }
        val contents = Backup.contents(backup)
        if (quiet && contents == lastWritten) return
        backupBusy = true
        if (!quiet) showBackup("Backing up…")
        val text = Backup.text(backup)
        val folder = backupFolder
        scope.launch {
            val written = withContext(Dispatchers.IO) {
                runCatching {
                    val (place, words) = folder()
                    Backup.write(text, File(place, Backup.FILE_NAME))
                    words
                }
            }
            backupBusy = false
            written.fold(
                onSuccess = { place ->
                    lastWritten = contents
                    backupPlace = place
                    runCatching { store.put(Backup.LAST_KEY, now()) }
                    showBackup()
                },
                onFailure = { showBackup("Could not back up: ${it.message}.") },
            )
        }
    }

    /** Reads the backup and adds what is missing here. */
    fun restore() {
        if (backupBusy) return
        backupBusy = true
        showBackup("Looking for the backup…")
        val folder = backupFolder
        scope.launch {
            val read = withContext(Dispatchers.IO) {
                val (place, words) = folder()
                val file = File(place, Backup.FILE_NAME)
                runCatching { file.readText() }.fold(
                    onSuccess = { Result.success(it) },
                    onFailure = { Result.failure(Exception("There is no backup in ${words.removeSuffix(" on this phone")} yet")) },
                )
            }
            backupBusy = false
            val text = read.getOrElse { return@launch showBackup("${it.message}.") }
            val backup = try {
                Backup.read(text)
            } catch (e: Exception) {
                return@launch showBackup("That file is not a Haven backup.")
            }
            try {
                val merged = Backup.restore(backup, store, sources = addonScreens)
                if (merged.tracking) tracker.reloadAccounts()
                library.loadServers()
                library.reloadWatchlist()
                library.marked()
                library.title.refreshTicks()
                library.loadSources()
                library.reloadHome()
                live.rebuild()
                keys = runCatching { Keys.load(store) }.getOrDefault(keys)
                changed()
                val count = merged.added
                showBackup(
                    "Restored the backup from ${Backup.`when`(Backup.madeAt(backup))}: $count new ${if (count == 1) "thing" else "things"}. " +
                        "New TV sources and settings show the next time Haven starts.",
                )
            } catch (e: Exception) {
                showBackup("Could not restore the backup: ${e.message}")
            }
        }
    }

    // About

    val dataFolder: String get() = paths.dataDir.path
    val logFile: String get() = paths.logFile.path

    companion object {
        const val NEW_ISSUE = "https://github.com/TinusTK/Haven/issues/new"
        const val SOURCE = "https://github.com/TinusTK/Haven"
    }
}
