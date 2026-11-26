package net.haven.app.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import kotlinx.coroutines.launch
import net.haven.app.BuildConfig
import net.haven.app.R
import net.haven.app.app.AppModel
import net.haven.app.app.SettingsModel
import net.haven.app.app.SettingsModel.Section
import net.haven.app.core.Backup
import net.haven.app.library.DownloadRules
import net.haven.app.servers.ServerAccounts
import net.haven.app.servers.ServerKind
import net.haven.app.tracking.Tracker
import net.haven.app.tracking.TrackingService

/** Settings: a list of parts under their groups; pressing one opens it.
 *  Pressing Settings in the tab bar again (or Back) goes back to the list. */
@Composable
fun SettingsPage(model: AppModel) {
    val settings = model.settingsPage
    val palette = LocalPalette.current
    val tv = LocalTV.current
    LaunchedEffect(model.settingsBackToList) { settings.section = null }
    // On a TV the remote goes to the part just opened (its Back button), and
    // back to the part's row when it closes.
    val back = remember { FocusRequester() }
    val rows = remember { Section.entries.associateWith { FocusRequester() } }
    var last by remember { mutableStateOf<Section?>(null) }
    LaunchedEffect(settings.section) {
        val section = settings.section
        if (tv) runCatching { if (section != null) back.requestFocus() else last?.let { rows[it]?.requestFocus() } }
        if (section != null) last = section
    }
    BackHandler(enabled = settings.section != null) { settings.section = null }
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState())
            .padding(start = if (tv) 0.dp else 16.dp, end = if (tv) 0.dp else 16.dp, top = 12.dp, bottom = 32.dp)
            .then(if (tv) Modifier.widthIn(max = 900.dp) else Modifier),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        val section = settings.section
        if (section != null) {
            Chip("‹  Settings", false, Modifier.focusRequester(back).semantics { contentDescription = "Back to Settings" }) { settings.section = null }
            PageTitle(section.label)
            when (section) {
                Section.General -> GeneralSettings(model, settings)
                Section.Storage -> StorageSettings(model, settings)
                Section.Accounts -> AccountSettings(settings, model.tracker)
                Section.Sync -> SyncSettings(settings, model.tracker)
                Section.Servers -> ServerSettings(settings)
                Section.Addons -> if (settings.addonScreens) AddonSettings(settings)
                Section.LiveTV -> LiveTVSettings(settings)
                Section.About -> AboutSettings(settings)
            }
        } else {
            PageTitle("Settings")
            Section.groups(settings.addonScreens).forEach { (name, sections) ->
                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(
                        name.uppercase(), Modifier.padding(start = 4.dp).semantics { heading() },
                        style = Type.font(if (tv) 14.sp else 11.sp, FontWeight.Bold, palette.textDim),
                    )
                    val shape = RoundedCornerShape(14.dp)
                    Column(Modifier.fillMaxWidth().background(palette.surface, shape).border(1.dp, palette.border, shape)) {
                        sections.forEachIndexed { at, item ->
                            Row(
                                Modifier.fillMaxWidth().height(if (tv) 60.dp else 50.dp)
                                    .focusRequester(rows.getValue(item))
                                    .pressable(RoundedCornerShape(12.dp)) { settings.section = item }
                                    .padding(horizontal = 14.dp),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(12.dp),
                            ) {
                                HavenIcon(item.icon, 20.dp, palette.accent)
                                Text(item.label, Modifier.weight(1f), style = Type.font(if (tv) 18.sp else 15.sp, FontWeight.SemiBold, palette.text))
                                HavenIcon("forward", 14.dp, palette.textDim)
                            }
                            if (at < sections.size - 1) Box(Modifier.padding(start = 46.dp).fillMaxWidth().height(1.dp).background(palette.border))
                        }
                    }
                }
            }
        }
    }
}

// Building blocks
//
// Buttons stay pressable while their work runs (the model ignores a second
// press): a button that goes grey under the remote drops the focus, and the
// next arrow press then lands on the tabs.

/** A rounded card with a small heading. */
@Composable
private fun SettingsCard(title: String? = null, content: @Composable ColumnScope.() -> Unit) {
    val palette = LocalPalette.current
    val shape = RoundedCornerShape(14.dp)
    Column(
        Modifier.fillMaxWidth().background(palette.surface, shape).border(1.dp, palette.border, shape).padding(14.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        if (title != null) {
            Text(title.uppercase(), Modifier.semantics { heading() }, style = Type.font(if (LocalTV.current) 14.sp else 11.sp, FontWeight.Bold, palette.textDim))
        }
        content()
    }
}

/** A label with a line under it, and something on the right. */
@Composable
private fun SettingsRow(label: String, detail: String = "", trailing: @Composable () -> Unit = {}) {
    val palette = LocalPalette.current
    val tv = LocalTV.current
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(label, style = Type.font(if (tv) 18.sp else 15.sp, FontWeight.SemiBold, palette.text))
            if (detail.isNotEmpty()) Text(detail, style = Type.font(if (tv) 15.sp else 12.sp, color = palette.textDim))
        }
        trailing()
    }
}

/** Small text under something. */
@Composable
private fun Note(text: String, error: Boolean = false) {
    val palette = LocalPalette.current
    Text(text, style = Type.font(if (LocalTV.current) 16.sp else 13.sp, color = if (error) palette.alert else palette.textDim))
}

/** A switch in Haven's gold; the remote can reach it. */
@Composable
private fun HavenSwitch(label: String, on: Boolean, set: (Boolean) -> Unit) {
    val palette = LocalPalette.current
    val shape = RoundedCornerShape(50)
    Box(
        Modifier.width(48.dp).height(28.dp)
            .background(if (on) palette.accent else palette.border, shape)
            .pressable(shape, role = Role.Switch) { set(!on) }
            .semantics {
                contentDescription = label
                stateDescription = if (on) "On" else "Off"
            }
            .padding(3.dp),
        contentAlignment = if (on) Alignment.CenterEnd else Alignment.CenterStart,
    ) {
        Box(Modifier.size(22.dp).background(if (on) palette.onAccent else palette.textDim, CircleShape))
    }
}

/** A round dot: green online, red offline, grey not known. */
@Composable
private fun StateDot(online: Boolean?) {
    val palette = LocalPalette.current
    Box(
        Modifier.size(9.dp)
            .background(if (online == true) Color(0xFF3CC37A) else if (online == false) palette.alert else palette.textDim.copy(alpha = 0.5f), CircleShape)
            .clearAndSetSemantics { contentDescription = if (online == true) "Online" else if (online == false) "Offline" else "Checking" },
    )
}

/** Asks before something is removed. */
@Composable
private fun RemoveButton(name: String, what: String, remove: () -> Unit) {
    var asking by remember { mutableStateOf(false) }
    Chip("Remove", false, Modifier.semantics { contentDescription = "Remove $name" }) { asking = true }
    if (asking) {
        val palette = LocalPalette.current
        Dialog(onDismissRequest = { asking = false }) {
            Column(
                Modifier.fillMaxWidth().background(palette.background, RoundedCornerShape(18.dp)).padding(20.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text("Remove $name?", style = Type.heading(20.sp, palette.text))
                Text(what, style = Type.font(14.sp, color = palette.textDim))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Chip("Cancel", false) { asking = false }
                    Chip("Remove", true) {
                        asking = false
                        remove()
                    }
                }
            }
        }
    }
}

/** One of a list, chosen in a box that opens over the page. */
@Composable
private fun ChoiceButton(label: String, choices: List<String>, current: Int, choose: (Int) -> Unit) {
    var open by remember { mutableStateOf(false) }
    val palette = LocalPalette.current
    Chip("${choices.getOrNull(current) ?: ""}  ›", false, Modifier.semantics { contentDescription = "$label: ${choices.getOrNull(current) ?: ""}" }) { open = true }
    if (open) {
        Dialog(onDismissRequest = { open = false }) {
            Column(
                Modifier.fillMaxWidth().heightIn(max = 560.dp).background(palette.background, RoundedCornerShape(18.dp)).padding(20.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text(label, style = Type.heading(20.sp, palette.text))
                Column(Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    choices.forEachIndexed { at, name ->
                        val chosen = at == current
                        val shape = RoundedCornerShape(10.dp)
                        Row(
                            Modifier.fillMaxWidth().heightIn(min = 44.dp)
                                .background(if (chosen) palette.selected else Color.Transparent, shape)
                                .pressable(shape) {
                                    open = false
                                    choose(at)
                                }
                                .padding(horizontal = 12.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(name, Modifier.weight(1f), style = Type.font(15.sp, if (chosen) FontWeight.Bold else FontWeight.Normal, palette.text))
                            if (chosen) HavenIcon("check", 16.dp, palette.accent)
                        }
                    }
                }
            }
        }
    }
}

// General

@Composable
private fun GeneralSettings(model: AppModel, settings: SettingsModel) {
    SettingsCard("Appearance") {
        SettingsRow("Dark theme", "Switch between the dark and light look.") {
            HavenSwitch("Dark theme", model.dark) { model.setDark(it) }
        }
    }
    SettingsCard("Where to watch") {
        SettingsRow(
            "Country",
            "Set from this device's region unless you pick one. A title's page lists the streaming services and shops that have it in this country; if none does, Haven looks in the United States, the United Kingdom, Canada and Australia.",
        )
        ChoiceButton("Country", settings.countries, settings.country) { settings.setCountry(it) }
    }
    SettingsCard("Player") {
        SettingsRow("Play videos with")
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            listOf("Haven's own player", "Another app").forEachIndexed { at, label ->
                Chip(label, settings.otherPlayer == at) { settings.setOtherPlayer(at) }
            }
        }
        Note(
            "With another app, videos open in a player app on this device, such as VLC or mpv; Android asks which one the first time. " +
                "Downloads and the videos in Library › Local always play in Haven's own player, and resuming, Continue Watching and the next episode only work there.",
        )
        SettingsRow("Send surround sound to a receiver untouched", "For an AV receiver or soundbar on HDMI: needed for Dolby Atmos and DTS:X.") {
            HavenSwitch("Send surround sound to a receiver untouched", settings.passthrough) { settings.setPassthrough(it) }
        }
        Note(
            "Off, Haven decodes the sound itself and plays it on whatever this device has, in surround where the device can. On, Dolby Digital, Dolby TrueHD and DTS sound is passed on as it is in the file for the receiver to decode. Turn it on only with a receiver or soundbar that takes those; without one there may be no sound.",
        )
        SettingsRow("Let the volume go up to 200%", "For quiet films: the player's volume goes past 100%, made louder by Haven. Very loud sound can crackle.") {
            HavenSwitch("Let the volume go up to 200%", settings.loud) { settings.setLoud(it) }
        }
    }
}

// Downloads & storage

@Composable
private fun StorageSettings(model: AppModel, settings: SettingsModel) {
    settings.revision
    SettingsCard("Downloads") {
        SettingsRow("Downloads folder", DownloadRules.placeWords(model.core.downloads))
        SettingsRow("Downloads", "${model.library.downloads.rows.size} saved or on their way.") {
            Chip("Open in Library", false) { model.openLibrary(1) }
        }
    }
    SettingsCard("Storage") {
        SettingsRow("Picture cache", settings.cacheText()) {
            Chip("Clear cache", false) { settings.clearCache() }
        }
        Note("Posters and other pictures are kept on this device once fetched, so they show at once the next time. Clearing the cache only removes those copies; they are fetched again when needed.")
    }
}

// Accounts

@Composable
private fun AccountSettings(settings: SettingsModel, tracker: Tracker) {
    settings.revision
    val palette = LocalPalette.current
    val scope = rememberCoroutineScope()
    SettingsCard("Tracking") {
        TrackingService.entries.forEach { service ->
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                SettingsRow(service.label, tracker.rowText(service))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (tracker.account(service) != null) {
                        Chip("Import watchlist", false) { scope.launch { tracker.importWatchlist(service) } }
                        Chip("Unlink", false) { tracker.disconnect(service) }
                    } else if (tracker.client.hasKeys(service) && !tracker.linking) {
                        Chip("Link", true) { tracker.connect(service) }
                    }
                }
            }
        }
        if (tracker.linking) {
            if (tracker.code.isNotEmpty()) {
                Text("Code: ${tracker.code}", style = Type.font(22.sp, FontWeight.ExtraBold, palette.accent).copy(fontFamily = FontFamily.Monospace))
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (tracker.link.isNotEmpty()) Chip("Open the sign-in page again", false) { settings.openLink(tracker.link) }
                Chip("Cancel", false) { tracker.cancelLinking() }
            }
        }
        if (tracker.linkStatus.isNotEmpty()) Note(tracker.linkStatus)
    }
    StremioCard(settings)
    SettingsCard("Your own keys") {
        KeyField("TMDB API key", settings.keys.tmdb) { settings.keys = settings.keys.copy(tmdb = it) }
        KeyField("Trakt client ID", settings.keys.traktId) { settings.keys = settings.keys.copy(traktId = it) }
        KeyField("Trakt client secret", settings.keys.traktSecret, secret = true) { settings.keys = settings.keys.copy(traktSecret = it) }
        KeyField("Simkl client ID", settings.keys.simklId) { settings.keys = settings.keys.copy(simklId = it) }
        KeyField("OMDb API key", settings.keys.omdb, placeholder = "For Rotten Tomatoes scores (omdbapi.com)") { settings.keys = settings.keys.copy(omdb = it) }
        PrimaryButton("Save keys") { settings.saveKeys() }
        if (settings.keysStatus.isNotEmpty()) Note(settings.keysStatus)
    }
}

@Composable
private fun KeyField(label: String, text: String, secret: Boolean = false, placeholder: String? = null, change: (String) -> Unit) {
    val palette = LocalPalette.current
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(label, style = Type.font(13.sp, FontWeight.SemiBold, palette.text))
        HavenField(text, change, placeholder ?: label, Modifier.fillMaxWidth().semantics { contentDescription = label }, secret = secret)
    }
}

/** Signing in to a Stremio account brings in the add-ons it uses. */
@Composable
private fun StremioCard(settings: SettingsModel) {
    SettingsCard("Stremio") {
        if (settings.stremioEmail.isEmpty()) {
            Note("Sign in with your Stremio account and Haven uses the add-ons it has, asking them itself as Stremio does. Your password is not kept.")
            HavenField(settings.stremioEmailEntered, { settings.stremioEmailEntered = it }, "Email address", Modifier.fillMaxWidth())
            HavenField(
                settings.stremioPasswordEntered, { settings.stremioPasswordEntered = it }, "Password",
                Modifier.fillMaxWidth(), onSubmit = { settings.stremioSignIn() }, secret = true,
            )
            PrimaryButton(if (settings.stremioBusy) "Signing in…" else "Sign in") { settings.stremioSignIn() }
        } else {
            SettingsRow("Signed in", "${settings.stremioEmail}. The add-ons on this account are used, and follow it each time Haven starts.")
            Chip("Sign out", false) { settings.stremioSignOut() }
        }
        if (settings.stremioStatus.isNotEmpty()) Note(settings.stremioStatus)
    }
}

// Sync options

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun SyncSettings(settings: SettingsModel, tracker: Tracker) {
    settings.revision
    val palette = LocalPalette.current
    val status = tracker.status
    SettingsCard("On this device") {
        SettingsRow("Local (works offline)", "What you watch and your Watchlist are always kept here first, with or without a connection, and sent on to the accounts below.") {
            Text("On", style = Type.font(13.sp, FontWeight.Bold, palette.accent))
        }
    }
    SettingsCard("Your accounts") {
        Note(status.text, status.problem)
        status.rows.forEach { row ->
            SettingsRow("Sync with ${row.service.label}", row.detail) {
                if (row.linked) {
                    HavenSwitch("Sync with ${row.service.label}", row.on) { tracker.setSync(row.service, it) }
                } else {
                    Chip("Link  ›", false) { settings.section = Section.Accounts }
                }
            }
        }
        if (status.linked) Chip(if (status.busy) "Syncing…" else "Sync now", false) { if (!status.busy) tracker.syncNow() }
        Note(Tracker.SYNC_NOTE)
    }
    SettingsCard("Backup") {
        SettingsRow(Backup.WHERE_WORDS, settings.backupStatus)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Chip(if (settings.backupBusy) "Working…" else "Back up now", true) { settings.backUp() }
            Chip("Restore", false) { settings.restore() }
        }
        Note("Restore puts back anything missing on this device and removes nothing.")
    }
}

// Media servers

@Composable
private fun ServerSettings(settings: SettingsModel) {
    val palette = LocalPalette.current
    val tv = LocalTV.current
    LaunchedEffect(Unit) { settings.refresh() }
    SettingsCard {
        Note("Sign in to your own Jellyfin, Emby or Plex server.")
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            ServerKind.entries.forEach { kind -> Chip(kind.label, settings.serverKind == kind) { settings.serverKind = kind } }
        }
        if (settings.serverKind != ServerKind.Plex) {
            HavenField(settings.serverAddress, { settings.serverAddress = it }, "Server address, for example 192.168.1.10:8096", Modifier.fillMaxWidth())
            HavenField(settings.serverUser, { settings.serverUser = it }, "User name", Modifier.fillMaxWidth())
            HavenField(settings.serverPassword, { settings.serverPassword = it }, "Password", Modifier.fillMaxWidth(), onSubmit = { settings.signIn(tv) }, secret = true)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            PrimaryButton(if (settings.serverKind == ServerKind.Plex) "Sign in with Plex" else "Sign in") { settings.signIn(tv) }
            if (settings.signingIn) Chip("Cancel", false) { settings.cancelSignIn() }
        }
        if (settings.plexCode.isNotEmpty()) {
            Text("Code: ${settings.plexCode}", style = Type.font(20.sp, FontWeight.ExtraBold, palette.accent).copy(fontFamily = FontFamily.Monospace))
            if (!tv) Chip("Open the Plex page again", false) { settings.openLink(settings.plexLink) }
        }
        settings.serverStatus?.let { Note(it.text, it.isError) }
    }
    SettingsCard("Your servers") {
        val servers = settings.servers
        if (servers.isEmpty()) Note("No media servers added yet.")
        servers.forEachIndexed { at, server ->
            val on = settings.serverOn(server)
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (on) {
                        val state = settings.serverState(server)
                        StateDot(if (state == ServerAccounts.State.Unknown) null else state == ServerAccounts.State.Online)
                    }
                    Box(Modifier.weight(1f)) {
                        SettingsRow(server.name, "${server.kind.label} · ${server.address}") {
                            HavenSwitch("Use ${server.name}", on) { settings.setServer(server, it) }
                        }
                    }
                }
                RemoveButton(server.name, "Haven forgets its sign-in.") { settings.removeServer(server) }
            }
            if (at < servers.size - 1) Box(Modifier.fillMaxWidth().height(1.dp).background(palette.border))
        }
        if (servers.isNotEmpty()) Chip("Check again", false) { settings.checkServers() }
    }
}

// Add-ons (GitHub build)

@Composable
private fun AddonSettings(settings: SettingsModel) {
    SettingsCard {
        Note("Paste an add-on's address to install it. Its catalogues show up as shelves on Home, next to Cinemeta, the free catalogue of films and series built into Haven.")
        HavenField(settings.addonAddress, { settings.addonAddress = it }, "https://…/manifest.json", Modifier.fillMaxWidth().semantics { contentDescription = "Add-on address" }, onSubmit = { settings.installAddon() })
        PrimaryButton(if (settings.addonBusy) "Installing…" else "Install") { settings.installAddon() }
        settings.addonStatus?.let { Note(it.message, it.isError) }
    }
    SettingsCard("Installed") {
        val rows = settings.addons
        if (rows.isEmpty()) Note("No add-ons installed yet.")
        rows.forEach { row ->
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                SettingsRow(row.name, listOf(if (row.version.isEmpty()) "" else "Version ${row.version}", row.description).filter { it.isNotEmpty() }.joinToString(" · ")) {
                    HavenSwitch("Use ${row.name}", row.enabled) { settings.setAddon(row.url, it) }
                }
                RemoveButton(row.name, "Its catalogues leave Home.") { settings.removeAddon(row.url) }
            }
        }
    }
}

// Live TV

@Composable
private fun LiveTVSettings(settings: SettingsModel) {
    SettingsCard {
        Note("Add your TV provider's channels, either as the address of an M3U playlist or as an Xtream Codes account (server address, user name and password). The channels then show on the Live TV tab, and radio stations on Radio.")
    }
    SettingsCard("M3U playlist") {
        HavenField(
            settings.playlistAddress, { settings.playlistAddress = it }, "M3U playlist address, for example https://…/playlist.m3u",
            Modifier.fillMaxWidth().semantics { contentDescription = "M3U playlist address" }, onSubmit = { settings.addPlaylist() },
        )
        PrimaryButton("Add playlist") { settings.addPlaylist() }
        settings.tvStatus?.takeIf { !it.account }?.let { Note(it.text, it.isError) }
    }
    SettingsCard("Xtream Codes account") {
        HavenField(settings.xtreamServer, { settings.xtreamServer = it }, "Server address, for example http://tv.example.com:8080", Modifier.fillMaxWidth().semantics { contentDescription = "Xtream Codes server address" })
        HavenField(settings.xtreamUser, { settings.xtreamUser = it }, "User name", Modifier.fillMaxWidth().semantics { contentDescription = "Xtream Codes user name" })
        HavenField(
            settings.xtreamPassword, { settings.xtreamPassword = it }, "Password",
            Modifier.fillMaxWidth().semantics { contentDescription = "Xtream Codes password" }, onSubmit = { settings.addXtream() }, secret = true,
        )
        PrimaryButton("Add account") { settings.addXtream() }
        settings.tvStatus?.takeIf { it.account }?.let { Note(it.text, it.isError) }
    }
    SettingsCard("Your sources") {
        val rows = settings.tvSources
        if (rows.isEmpty()) Note("No Live TV sources added yet.")
        rows.forEach { row ->
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (row.isAccount && row.enabled) StateDot(settings.tvOnline(row))
                    Box(Modifier.weight(1f)) {
                        SettingsRow(row.name, "${row.detail} · ${row.channels} channel${if (row.channels == 1) "" else "s"}") {
                            HavenSwitch("Use ${row.name}", row.enabled) { settings.setTVSource(row.url, it) }
                        }
                    }
                }
                RemoveButton(row.name, "Its channels leave Live TV.") { settings.removeTVSource(row.url) }
            }
        }
    }
}

// About

@Composable
private fun AboutSettings(settings: SettingsModel) {
    val palette = LocalPalette.current
    var licences by remember { mutableStateOf(false) }
    SettingsCard {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Image(painterResource(R.drawable.halo), null, Modifier.size(44.dp))
            Text("Haven", style = Type.heading(24.sp, palette.text))
        }
        SettingsRow("Version", "${BuildConfig.VERSION_NAME} (build ${BuildConfig.HAVEN_BUILD})${if (settings.addonScreens) " · GitHub build" else ""}")
        SettingsRow("Data folder", settings.dataFolder)
        SettingsRow("Log file", settings.logFile)
    }
    SettingsCard {
        Note("Something not working? Tell us on GitHub.")
        PrimaryButton("Report an issue") { settings.openLink(SettingsModel.NEW_ISSUE) }
    }
    SettingsCard {
        Text(CREDITS, style = Type.font(13.sp, color = palette.text))
        Chip(if (licences) "Hide the licences" else "Licences of the parts Haven uses", false) { licences = !licences }
        if (licences) Text(LICENCES, style = Type.font(11.sp, color = palette.textDim).copy(fontFamily = FontFamily.Monospace))
    }
    Spacer(Modifier.height(8.dp))
}

private const val CREDITS = """Haven: a media player and tracker for the films and TV shows you love.

Haven © 2026 TinusTK. Free open source software under the GNU GPL version 3.
Video playback by mpv (libmpv, LGPL). Database by SQLite (public domain).

Data sources
Film and TV information from TMDB and Cinemeta. Tracking with Trakt and Simkl. Where to watch by JustWatch.
This product uses the TMDB API but is not endorsed or certified by TMDB."""

private const val LICENCES = """mpv and libmpv: LGPL 2.1 or later (https://mpv.io).
FFmpeg: LGPL 2.1 or later (https://ffmpeg.org).
SQLite: public domain (https://sqlite.org).
OkHttp, Kotlin, kotlinx.coroutines, kotlinx.serialization, AndroidX and Jetpack Compose: Apache License 2.0 (https://www.apache.org/licenses/LICENSE-2.0).
Google Cast SDK: Google APIs Terms of Service."""
