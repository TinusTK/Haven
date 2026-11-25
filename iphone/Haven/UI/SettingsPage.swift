import SwiftUI

/// Settings: a list of parts under their groups; tapping one opens it.
/// Tapping Settings in the tab bar again goes back to the list.
struct SettingsPage: View {
    let model: AppModel
    @Bindable var settings: SettingsModel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                if let section = settings.section {
                    Chip(label: "‹  Settings") { settings.section = nil }
                        .accessibilityLabel("Back to Settings")
                    PageTitle(text: section.label)
                    content(section)
                } else {
                    PageTitle(text: "Settings")
                    ForEach(SettingsModel.Section.groups, id: \.name) { group in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(group.name.uppercased())
                                .font(Palette.font(11, .bold))
                                .foregroundStyle(Palette.textDim)
                                .padding(.leading, 4)
                                .accessibilityAddTraits(.isHeader)
                            VStack(spacing: 0) {
                                ForEach(group.sections) { section in
                                    Button { settings.section = section } label: {
                                        HStack(spacing: 12) {
                                            HavenIcon(name: section.icon, size: 20, tint: Palette.accent)
                                            Text(section.label).font(Palette.font(15, .semibold)).foregroundStyle(Palette.text)
                                            Spacer()
                                            HavenIcon(name: "forward", size: 14, tint: Palette.textDim)
                                        }
                                        .padding(.horizontal, 14)
                                        .frame(height: 50)
                                        .contentShape(Rectangle())
                                    }
                                    .buttonStyle(PressedStyle())
                                    if section != group.sections.last {
                                        Rectangle().fill(Palette.border).frame(height: 1).padding(.leading, 46)
                                    }
                                }
                            }
                            .background(RoundedRectangle(cornerRadius: 14).fill(Palette.surface))
                            .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(Palette.border, lineWidth: 1))
                        }
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16)
            .padding(.top, 12)
            .padding(.bottom, 32)
        }
        .scrollDismissesKeyboard(.immediately)
        .onChange(of: model.settingsBackToList) { _, _ in settings.section = nil }
    }

    @ViewBuilder private func content(_ section: SettingsModel.Section) -> some View {
        switch section {
        case .general: GeneralSettings(model: model, settings: settings)
        case .storage: StorageSettings(model: model, settings: settings)
        case .accounts: AccountSettings(settings: settings, tracker: model.tracker)
        case .sync: SyncSettings(settings: settings, tracker: model.tracker)
        case .servers: ServerSettings(settings: settings)
        case .addons:
            #if ADDONS
            AddonSettings(settings: settings)
            #else
            EmptyView()
            #endif
        case .liveTV: LiveTVSettings(settings: settings)
        case .about: AboutSettings(settings: settings)
        }
    }
}

// MARK: Building blocks

/// A rounded card with a small heading.
struct SettingsCard<Content: View>: View {
    var title: String?
    @ViewBuilder var content: () -> Content

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if let title {
                Text(title.uppercased())
                    .font(Palette.font(11, .bold))
                    .foregroundStyle(Palette.textDim)
                    .accessibilityAddTraits(.isHeader)
            }
            content()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(14)
        .background(RoundedRectangle(cornerRadius: 14).fill(Palette.surface))
        .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(Palette.border, lineWidth: 1))
    }
}

/// A label with a line under it, and something on the right.
struct SettingsRow<Trailing: View>: View {
    let label: String
    var detail = ""
    @ViewBuilder var trailing: () -> Trailing

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(label).font(Palette.font(15, .semibold)).foregroundStyle(Palette.text)
                if !detail.isEmpty {
                    Text(detail).font(Palette.font(12)).foregroundStyle(Palette.textDim).fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer(minLength: 0)
            trailing()
        }
    }
}

/// Small text under something.
struct Note: View {
    let text: String
    var error = false

    var body: some View {
        Text(text)
            .font(Palette.font(13))
            .foregroundStyle(error ? Palette.alert : Palette.textDim)
            .fixedSize(horizontal: false, vertical: true)
    }
}

/// A switch in Haven's gold.
struct HavenToggle: View {
    let label: String
    let on: Bool
    let set: (Bool) -> Void

    var body: some View {
        Toggle(label, isOn: Binding(get: { on }, set: set))
            .labelsHidden()
            .tint(Palette.accent)
            .accessibilityLabel(label)
    }
}

/// A round dot: green online, red offline, grey not known.
private struct StateDot: View {
    let online: Bool?

    var body: some View {
        Circle()
            .fill(online == true ? Color.green : online == false ? Palette.alert : Palette.textDim.opacity(0.5))
            .frame(width: 9, height: 9)
            .accessibilityLabel(online == true ? "Online" : online == false ? "Offline" : "Checking")
    }
}

/// Asks before something is removed.
private struct RemoveButton: View {
    let name: String
    let what: String
    let remove: () -> Void
    @State private var asking = false

    var body: some View {
        Chip(label: "Remove") { asking = true }
            .accessibilityLabel("Remove \(name)")
            .confirmationDialog("Remove \(name)?", isPresented: $asking, titleVisibility: .visible) {
                Button("Remove", role: .destructive, action: remove)
            } message: {
                Text(what)
            }
    }
}

// MARK: General

private struct GeneralSettings: View {
    let model: AppModel
    let settings: SettingsModel

    var body: some View {
        SettingsCard(title: "Appearance") {
            SettingsRow(label: "Dark theme", detail: "Switch between the dark and light look.") {
                HavenToggle(label: "Dark theme", on: model.dark) { model.setDark($0) }
            }
        }
        SettingsCard(title: "Where to watch") {
            SettingsRow(label: "Country",
                        detail: "Set from this device's region unless you pick one. A title's page lists the streaming services and shops that have it in this country; if none does, Haven looks in the United States, the United Kingdom, Canada and Australia.") {
                EmptyView()
            }
            Picker("Country", selection: Binding(get: { settings.country }, set: { settings.setCountry($0) })) {
                ForEach(Array(settings.countries.enumerated()), id: \.offset) { index, name in
                    Text(name).tag(index)
                }
            }
            .pickerStyle(.menu)
            .tint(Palette.accent)
        }
        SettingsCard(title: "Player") {
            SettingsRow(label: "Play videos with") { EmptyView() }
            Picker("Play videos with", selection: Binding(get: { settings.otherPlayer }, set: { settings.setOtherPlayer($0) })) {
                ForEach(Array(PlayerRules.OtherPlayer.names.enumerated()), id: \.offset) { index, name in
                    Text(name).tag(index)
                }
            }
            .pickerStyle(.menu)
            .tint(Palette.accent)
            Note(text: "Another player app (VLC, Infuse or Outplayer) must be installed. Downloads and the videos in Haven › Videos always play in Haven's own player, and resuming, Continue Watching and the next episode only work there.")
            SettingsRow(label: "Send surround sound to a receiver untouched",
                        detail: "For an AV receiver or soundbar: needed for Dolby Atmos and DTS:X.") {
                HavenToggle(label: "Send surround sound to a receiver untouched", on: settings.passthrough) { settings.setPassthrough($0) }
            }
            Note(text: "Off, Haven decodes the sound itself and plays it on whatever this iPhone has, in surround where the device can. On, Dolby Digital, Dolby TrueHD and DTS sound is passed on as it is in the file for the receiver to decode. Turn it on only with a receiver or soundbar that takes those; without one there may be no sound.")
        }
    }
}

// MARK: Downloads & storage

private struct StorageSettings: View {
    let model: AppModel
    let settings: SettingsModel

    var body: some View {
        SettingsCard(title: "Downloads") {
            SettingsRow(label: "Downloads folder", detail: DownloadRules.placeWords) { EmptyView() }
            SettingsRow(label: "Downloads", detail: "\(model.library.downloads.count) saved or on their way.") {
                Chip(label: "Open in Library") { model.openLibrary(1) }
            }
        }
        SettingsCard(title: "Storage") {
            SettingsRow(label: "Picture cache", detail: settings.cacheText) {
                Chip(label: "Clear cache") { settings.clearCache() }
            }
            Note(text: "Posters and other pictures are kept on this iPhone once fetched, so they show at once the next time. Clearing the cache only removes those copies; they are fetched again when needed.")
        }
    }
}

// MARK: Accounts

private struct AccountSettings: View {
    @Bindable var settings: SettingsModel
    let tracker: Tracker

    var body: some View {
        SettingsCard(title: "Tracking") {
            ForEach(TrackingService.allCases, id: \.self) { service in
                VStack(alignment: .leading, spacing: 8) {
                    SettingsRow(label: service.label, detail: tracker.rowText(service)) { EmptyView() }
                    HStack(spacing: 8) {
                        if tracker.account(service) != nil {
                            Chip(label: "Import watchlist") { Task { await tracker.importWatchlist(service) } }
                            Chip(label: "Unlink") { tracker.disconnect(service) }
                        } else if tracker.client.hasKeys(service) && !tracker.linking {
                            Chip(label: "Link", selected: true) { tracker.connect(service) }
                        }
                    }
                }
            }
            if tracker.linking {
                if !tracker.code.isEmpty {
                    Text("Code: \(tracker.code)")
                        .font(Palette.font(22, .heavy).monospaced())
                        .foregroundStyle(Palette.accent)
                        .textSelection(.enabled)
                }
                HStack(spacing: 8) {
                    if !tracker.link.isEmpty {
                        Chip(label: "Open the sign-in page again") { Browser.open(tracker.link) }
                    }
                    Chip(label: "Cancel") { tracker.cancelLinking() }
                }
            }
            if !tracker.linkStatus.isEmpty { Note(text: tracker.linkStatus) }
        }
        SettingsCard(title: "Your own keys") {
            key("TMDB API key", $settings.keys.tmdb)
            key("Trakt client ID", $settings.keys.traktID)
            key("Trakt client secret", $settings.keys.traktSecret, secure: true)
            key("Simkl client ID", $settings.keys.simklID)
            key("OMDb API key", $settings.keys.omdb, placeholder: "For Rotten Tomatoes scores (omdbapi.com)")
            PrimaryButton(text: "Save keys") { settings.saveKeys() }
            if !settings.keysStatus.isEmpty { Note(text: settings.keysStatus) }
        }
    }

    private func key(_ label: String, _ text: Binding<String>, secure: Bool = false, placeholder: String? = nil) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(label).font(Palette.font(13, .semibold)).foregroundStyle(Palette.text)
            HavenField(placeholder: placeholder ?? label, text: text, secure: secure)
                .accessibilityLabel(label)
        }
    }
}

// MARK: Sync options

private struct SyncSettings: View {
    let settings: SettingsModel
    let tracker: Tracker

    var body: some View {
        let status = tracker.status
        SettingsCard(title: "On this device") {
            SettingsRow(label: "Local (works offline)",
                        detail: "What you watch and your Watchlist are always kept here first, with or without a connection, and sent on to the accounts below.") {
                Text("On").font(Palette.font(13, .bold)).foregroundStyle(Palette.accent)
            }
        }
        SettingsCard(title: "Your accounts") {
            Note(text: status.text, error: status.problem)
            ForEach(status.rows, id: \.service) { row in
                SettingsRow(label: "Sync with \(row.service.label)", detail: row.detail) {
                    if row.linked {
                        HavenToggle(label: "Sync with \(row.service.label)", on: row.on) { tracker.setSync(row.service, on: $0) }
                    } else {
                        Chip(label: "Link  ›") { settings.section = .accounts }
                    }
                }
            }
            if status.linked {
                Chip(label: status.busy ? "Syncing…" : "Sync now") { tracker.syncNow() }
                    .disabled(status.busy)
            }
            Note(text: Tracker.syncNote)
        }
        SettingsCard(title: "Backup") {
            SettingsRow(label: Backup.whereWords, detail: settings.backupStatus) { EmptyView() }
            HStack(spacing: 8) {
                Chip(label: settings.backupBusy ? "Working…" : "Back up now", selected: true) { settings.backUp() }
                    .disabled(settings.backupBusy)
                Chip(label: "Restore") { settings.restore() }
                    .disabled(settings.backupBusy)
            }
            Note(text: "Restore puts back anything missing on this device and removes nothing.")
        }
    }
}

// MARK: Media servers

private struct ServerSettings: View {
    @Bindable var settings: SettingsModel

    var body: some View {
        SettingsCard {
            Note(text: "Sign in to your own Jellyfin, Emby or Plex server.")
            if Build.isStore {
                Text("Only works over Local Area Network")
                    .font(Palette.font(12, .bold))
                    .foregroundStyle(Palette.onAccent)
                    .padding(.horizontal, 10)
                    .frame(height: 24)
                    .background(Capsule().fill(Palette.accent))
            }
            Segments(labels: ["Jellyfin", "Emby", "Plex"],
                     current: Binding(get: { settings.serverKind.rawValue },
                                      set: { settings.serverKind = SettingsModel.ServerKindChoice(rawValue: $0) ?? .jellyfin }))
            if settings.serverKind != .plex {
                HavenField(placeholder: "Server address, for example 192.168.1.10:8096", text: $settings.serverAddress)
                    .accessibilityLabel("Server address")
                HavenField(placeholder: "User name", text: $settings.serverUser)
                    .accessibilityLabel("User name")
                HavenField(placeholder: "Password", text: $settings.serverPassword, secure: true, submit: { _ in settings.signIn() })
                    .accessibilityLabel("Password")
            }
            HStack(spacing: 8) {
                PrimaryButton(text: settings.serverKind == .plex ? "Sign in with Plex" : "Sign in", enabled: !settings.signingIn) {
                    settings.signIn()
                }
                if settings.signingIn { Chip(label: "Cancel") { settings.cancelSignIn() } }
            }
            if !settings.plexCode.isEmpty {
                Text("Code: \(settings.plexCode)").font(Palette.font(20, .heavy).monospaced()).foregroundStyle(Palette.accent)
                Chip(label: "Open the Plex page again") { Browser.open(settings.plexLink) }
            }
            if let status = settings.serverStatus { Note(text: status.text, error: status.isError) }
        }
        SettingsCard(title: "Your servers") {
            if settings.servers.isEmpty {
                Note(text: "No media servers added yet.")
            }
            ForEach(settings.servers, id: \.id) { server in
                let on = settings.serverOn(server)
                VStack(alignment: .leading, spacing: 8) {
                    HStack(spacing: 8) {
                        if on { StateDot(online: settings.serverState(server) == .unknown ? nil : settings.serverState(server) == .online) }
                        SettingsRow(label: server.name, detail: "\(server.kind.label) · \(server.address)") {
                            HavenToggle(label: "Use \(server.name)", on: on) { settings.setServer(server, on: $0) }
                        }
                    }
                    RemoveButton(name: server.name, what: "Haven forgets its sign-in.") { settings.removeServer(server) }
                }
                if server.id != settings.servers.last?.id { Rectangle().fill(Palette.border).frame(height: 1) }
            }
            if !settings.servers.isEmpty {
                Chip(label: "Check again") { settings.checkServers() }
            }
        }
    }
}

// MARK: Add-ons (GitHub build)

#if ADDONS
private struct AddonSettings: View {
    @Bindable var settings: SettingsModel

    var body: some View {
        SettingsCard {
            Note(text: "Paste an add-on's address to install it. Its catalogues show up as shelves on Home, next to Cinemeta, the free catalogue of films and series built into Haven.")
            HavenField(placeholder: "https://…/manifest.json", text: $settings.addonAddress, submit: { _ in settings.installAddon() })
                .accessibilityLabel("Add-on address")
            PrimaryButton(text: settings.addonBusy ? "Installing…" : "Install", enabled: !settings.addonBusy) { settings.installAddon() }
            if let status = settings.addonStatus { Note(text: status.message, error: status.isError) }
        }
        SettingsCard(title: "Installed") {
            if settings.addons.isEmpty { Note(text: "No add-ons installed yet.") }
            ForEach(settings.addons, id: \.url) { row in
                VStack(alignment: .leading, spacing: 8) {
                    SettingsRow(label: row.name, detail: [row.version.isEmpty ? "" : "Version \(row.version)", row.description]
                        .filter { !$0.isEmpty }.joined(separator: " · ")) {
                        HavenToggle(label: "Use \(row.name)", on: row.enabled) { settings.setAddon(row.url, on: $0) }
                    }
                    RemoveButton(name: row.name, what: "Its catalogues leave Home.") { settings.removeAddon(row.url) }
                }
            }
        }
    }
}
#endif

// MARK: Live TV

private struct LiveTVSettings: View {
    @Bindable var settings: SettingsModel

    var body: some View {
        SettingsCard {
            Note(text: "Add your TV provider's channels, either as the address of an M3U playlist or as an Xtream Codes account (server address, user name and password). The channels then show on the Live TV tab, and radio stations on Radio.")
        }
        SettingsCard(title: "M3U playlist") {
            HavenField(placeholder: "M3U playlist address, for example https://…/playlist.m3u", text: $settings.playlistAddress,
                       submit: { _ in settings.addPlaylist() })
                .accessibilityLabel("M3U playlist address")
            PrimaryButton(text: "Add playlist", enabled: !settings.tvBusy) { settings.addPlaylist() }
            if let status = settings.tvStatus, !status.account { Note(text: status.text, error: status.isError) }
        }
        SettingsCard(title: "Xtream Codes account") {
            HavenField(placeholder: "Server address, for example http://tv.example.com:8080", text: $settings.xtreamServer)
                .accessibilityLabel("Xtream Codes server address")
            HavenField(placeholder: "User name", text: $settings.xtreamUser)
                .accessibilityLabel("Xtream Codes user name")
            HavenField(placeholder: "Password", text: $settings.xtreamPassword, secure: true, submit: { _ in settings.addXtream() })
                .accessibilityLabel("Xtream Codes password")
            PrimaryButton(text: "Add account", enabled: !settings.tvBusy) { settings.addXtream() }
            if let status = settings.tvStatus, status.account { Note(text: status.text, error: status.isError) }
        }
        SettingsCard(title: "Your sources") {
            let rows = settings.tvSources
            if rows.isEmpty { Note(text: "No Live TV sources added yet.") }
            ForEach(rows, id: \.url) { row in
                VStack(alignment: .leading, spacing: 8) {
                    HStack(spacing: 8) {
                        if row.isAccount && row.enabled { StateDot(online: settings.tvOnline(row)) }
                        SettingsRow(label: row.name, detail: "\(row.detail) · \(row.channels) channel\(row.channels == 1 ? "" : "s")") {
                            HavenToggle(label: "Use \(row.name)", on: row.enabled) { settings.setTVSource(row.url, on: $0) }
                        }
                    }
                    RemoveButton(name: row.name, what: "Its channels leave Live TV.") { settings.removeTVSource(row.url) }
                }
            }
        }
    }
}

// MARK: About

private struct AboutSettings: View {
    let settings: SettingsModel
    @State private var licences = false

    var body: some View {
        SettingsCard {
            HStack(spacing: 12) {
                Image("Halo").resizable().scaledToFit().frame(width: 44, height: 44).accessibilityHidden(true)
                Text("Haven").font(Palette.heading(24)).foregroundStyle(Palette.text)
            }
            SettingsRow(label: "Version", detail: "\(Build.version) (\(Build.buildNumber))\(Build.isStore ? "" : " · GitHub build")") { EmptyView() }
            SettingsRow(label: "Data folder", detail: settings.dataFolder) { EmptyView() }
            SettingsRow(label: "Log file", detail: settings.logFile) { EmptyView() }
        }
        SettingsCard {
            Note(text: "Something not working? Tell us on GitHub.")
            PrimaryButton(text: "Report an issue") { Browser.open(SettingsModel.newIssue) }
        }
        SettingsCard {
            Text(Self.credits).font(Palette.font(13)).foregroundStyle(Palette.text).fixedSize(horizontal: false, vertical: true)
            Chip(label: licences ? "Hide the licences" : "Licences of the parts Haven uses") { licences.toggle() }
            if licences {
                Text(Self.grdbLicence).font(Palette.font(11).monospaced()).foregroundStyle(Palette.textDim)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    static let credits = """
        Haven: a media player and tracker for the films and TV shows you love.

        Haven © 2026 TinusTK. Free open source software under the GNU GPL version 3.
        Video playback by mpv (libmpv, LGPL). Database by GRDB (MIT licence).

        Data sources
        Film and TV information from TMDB and Cinemeta. Tracking with Trakt and Simkl. Where to watch by JustWatch.
        This product uses the TMDB API but is not endorsed or certified by TMDB.
        """

    static let grdbLicence = """
        GRDB.swift
        Copyright (C) 2015-2025 Gwendal Roué

        Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

        The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

        THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

        mpv and libmpv: LGPL 2.1 or later (https://mpv.io). FFmpeg: LGPL 2.1 or later (https://ffmpeg.org).
        """
}
