import SwiftUI

/// Settings on the television: the parts down the left, the chosen one on
/// the right. Sign-ins that need a web page (Trakt, Simkl, Plex) show a
/// code to enter on a phone or computer.
struct TVSettingsPage: View {
    let model: AppModel
    @Bindable var settings: SettingsModel

    /// Add-ons only in the GitHub build.
    static let sections: [SettingsModel.Section] = Build.hasAddons
        ? [.servers, .liveTV, .addons, .accounts, .sync, .general, .storage, .about]
        : [.servers, .liveTV, .accounts, .sync, .general, .storage, .about]

    private var shown: SettingsModel.Section { settings.section ?? .servers }

    var body: some View {
        HStack(alignment: .top, spacing: 60) {
            VStack(alignment: .leading, spacing: 12) {
                Text("Settings").font(Palette.heading(48)).foregroundStyle(Palette.text).padding(.bottom, 12)
                ForEach(Self.sections) { section in
                    TVRow(selected: section == shown) {
                        settings.section = section
                    } content: {
                        HavenIcon(name: section.icon, size: 30, tint: Palette.accent)
                        Text(section == .storage ? "Storage" : section.label)
                            .font(Palette.font(26, .semibold))
                            .foregroundStyle(Palette.text)
                    }
                }
                Spacer(minLength: 0)
            }
            .frame(width: 460)
            .focusSection()
            ScrollView {
                VStack(alignment: .leading, spacing: 30) {
                    content(shown)
                }
                .padding(8)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .scrollClipDisabled()
            .focusSection()
        }
        .padding(.horizontal, TV.side)
        .padding(.top, 40)
    }

    @ViewBuilder private func content(_ section: SettingsModel.Section) -> some View {
        switch section {
        case .servers: TVServerSettings(settings: settings)
        case .liveTV: TVLiveTVSettings(settings: settings)
        case .accounts: TVAccountSettings(settings: settings, tracker: model.tracker)
        case .sync: TVSyncSettings(settings: settings, tracker: model.tracker)
        case .general: TVGeneralSettings(model: model, settings: settings)
        case .storage:
            TVCard(title: "Storage") {
                TVNote(text: "Picture cache: \(settings.cacheText)")
                TVChip(label: "Clear cache") { settings.clearCache() }
                TVNote(text: "Posters and other pictures are kept on this Apple TV once fetched. Apple TV may also clear them itself when it runs short of space.")
            }
        case .addons:
            #if ADDONS
            TVAddonSettings(settings: settings)
            #else
            TVAboutSettings(settings: settings)
            #endif
        case .about: TVAboutSettings(settings: settings)
        }
    }
}

/// A part of a settings page with a small heading.
struct TVCard<Content: View>: View {
    var title: String?
    @ViewBuilder var content: () -> Content

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            if let title {
                Text(title.uppercased()).font(Palette.font(22, .bold)).foregroundStyle(Palette.textDim)
            }
            content()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(30)
        .background(RoundedRectangle(cornerRadius: 22).fill(Palette.surface))
        .overlay(RoundedRectangle(cornerRadius: 22).strokeBorder(Palette.border, lineWidth: 1))
    }
}

/// A text box for the remote (tvOS opens its own keyboard).
private struct TVField: View {
    let label: String
    @Binding var text: String
    var secure = false
    var submit: () -> Void = {}

    var body: some View {
        Group {
            if secure {
                SecureField(label, text: $text)
            } else {
                TextField(label, text: $text)
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)
            }
        }
        .font(Palette.font(26))
        .onSubmit(submit)
    }
}

/// A sign-in code to type in on another device.
private struct TVCode: View {
    let code: String
    let link: String

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            TVNote(text: "On your phone or computer, go to \(link.replacingOccurrences(of: "https://", with: "")) and enter:")
            Text(code).font(Palette.font(64, .heavy).monospaced()).foregroundStyle(Palette.accent)
        }
    }
}

private struct TVStateDot: View {
    let online: Bool?

    var body: some View {
        Circle()
            .fill(online == true ? Color.green : online == false ? Palette.alert : Palette.textDim.opacity(0.5))
            .frame(width: 18, height: 18)
    }
}

// MARK: Media servers

private struct TVServerSettings: View {
    @Bindable var settings: SettingsModel

    var body: some View {
        TVCard(title: "Sign in") {
            TVNote(text: Build.ownNetworkOnly
                ? "Sign in to your own Jellyfin, Emby or Plex server on your Local Area Network."
                : "Sign in to your own Jellyfin, Emby or Plex server.")
            HStack(spacing: 20) {
                ForEach(SettingsModel.ServerKindChoice.allCases, id: \.rawValue) { kind in
                    TVChip(label: ["Jellyfin", "Emby", "Plex"][kind.rawValue], selected: settings.serverKind == kind) {
                        settings.serverKind = kind
                    }
                }
            }
            .focusSection()
            if settings.serverKind != .plex {
                TVField(label: "Server address, for example 192.168.1.10:8096", text: $settings.serverAddress)
                TVField(label: "User name", text: $settings.serverUser)
                TVField(label: "Password", text: $settings.serverPassword, secure: true) { settings.signIn() }
            }
            HStack(spacing: 20) {
                TVChip(label: settings.serverKind == .plex ? "Sign in with Plex" : "Sign in", selected: true) { settings.signIn() }
                    .disabled(settings.signingIn)
                if settings.signingIn { TVChip(label: "Cancel") { settings.cancelSignIn() } }
            }
            if !settings.plexCode.isEmpty { TVCode(code: settings.plexCode, link: settings.plexLink) }
            if let status = settings.serverStatus { TVNote(text: status.text, error: status.isError) }
        }
        TVCard(title: "Your servers") {
            if settings.servers.isEmpty { TVNote(text: "No media servers added yet.") }
            ForEach(settings.servers, id: \.id) { server in
                let on = settings.serverOn(server)
                HStack(spacing: 20) {
                    if on {
                        let state = settings.serverState(server)
                        TVStateDot(online: state == .unknown ? nil : state == .online)
                    }
                    VStack(alignment: .leading, spacing: 4) {
                        Text(server.name).font(Palette.font(26, .semibold)).foregroundStyle(Palette.text)
                        Text("\(server.kind.label) · \(server.address)").font(Palette.font(20)).foregroundStyle(Palette.textDim)
                    }
                    Spacer(minLength: 0)
                    TVChip(label: on ? "On" : "Off", selected: on) { settings.setServer(server, on: !on) }
                    TVRemove(name: server.name, what: "Haven forgets its sign-in.") { settings.removeServer(server) }
                }
                .focusSection()
            }
            if !settings.servers.isEmpty { TVChip(label: "Check again") { settings.checkServers() } }
        }
    }
}

/// Asks before something is removed.
private struct TVRemove: View {
    let name: String
    let what: String
    let remove: () -> Void
    @State private var asking = false

    var body: some View {
        TVChip(label: "Remove") { asking = true }
            .confirmationDialog("Remove \(name)?", isPresented: $asking, titleVisibility: .visible) {
                Button("Remove", role: .destructive, action: remove)
            } message: {
                Text(what)
            }
    }
}

// MARK: Live TV

private struct TVLiveTVSettings: View {
    @Bindable var settings: SettingsModel

    var body: some View {
        TVCard(title: "M3U playlist") {
            TVField(label: "M3U playlist address, for example https://…/playlist.m3u", text: $settings.playlistAddress) {
                settings.addPlaylist()
            }
            TVChip(label: "Add playlist", selected: true) { settings.addPlaylist() }.disabled(settings.tvBusy)
            if let status = settings.tvStatus, !status.account { TVNote(text: status.text, error: status.isError) }
        }
        TVCard(title: "Xtream Codes account") {
            TVField(label: "Server address, for example http://tv.example.com:8080", text: $settings.xtreamServer)
            TVField(label: "User name", text: $settings.xtreamUser)
            TVField(label: "Password", text: $settings.xtreamPassword, secure: true) { settings.addXtream() }
            TVChip(label: "Add account", selected: true) { settings.addXtream() }.disabled(settings.tvBusy)
            if let status = settings.tvStatus, status.account { TVNote(text: status.text, error: status.isError) }
        }
        TVCard(title: "Your sources") {
            let rows = settings.tvSources
            if rows.isEmpty { TVNote(text: "No Live TV sources added yet.") }
            ForEach(rows, id: \.url) { row in
                HStack(spacing: 20) {
                    if row.isAccount && row.enabled { TVStateDot(online: settings.tvOnline(row)) }
                    VStack(alignment: .leading, spacing: 4) {
                        Text(row.name).font(Palette.font(26, .semibold)).foregroundStyle(Palette.text)
                        Text("\(row.detail) · \(row.channels) channel\(row.channels == 1 ? "" : "s")")
                            .font(Palette.font(20)).foregroundStyle(Palette.textDim)
                    }
                    Spacer(minLength: 0)
                    TVChip(label: row.enabled ? "On" : "Off", selected: row.enabled) { settings.setTVSource(row.url, on: !row.enabled) }
                    TVRemove(name: row.name, what: "Its channels leave Live TV.") { settings.removeTVSource(row.url) }
                }
                .focusSection()
            }
        }
    }
}

// MARK: Accounts

private struct TVAccountSettings: View {
    @Bindable var settings: SettingsModel
    let tracker: Tracker

    var body: some View {
        TVCard(title: "Tracking") {
            ForEach(TrackingService.allCases, id: \.self) { service in
                HStack(spacing: 20) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(service.label).font(Palette.font(26, .semibold)).foregroundStyle(Palette.text)
                        Text(tracker.rowText(service)).font(Palette.font(20)).foregroundStyle(Palette.textDim)
                    }
                    Spacer(minLength: 0)
                    if tracker.account(service) != nil {
                        TVChip(label: "Import watchlist") { Task { await tracker.importWatchlist(service) } }
                        TVChip(label: "Unlink") { tracker.disconnect(service) }
                    } else if tracker.client.hasKeys(service) && !tracker.linking {
                        TVChip(label: "Link", selected: true) { tracker.connect(service) }
                    }
                }
                .focusSection()
            }
            if tracker.linking {
                if !tracker.code.isEmpty { TVCode(code: tracker.code, link: tracker.link) }
                TVChip(label: "Cancel") { tracker.cancelLinking() }
            }
            if !tracker.linkStatus.isEmpty { TVNote(text: tracker.linkStatus) }
        }
        #if ADDONS
        TVStremioCard(settings: settings)
        #endif
        TVCard(title: "Your own keys") {
            TVField(label: "TMDB API key", text: $settings.keys.tmdb)
            TVField(label: "Trakt client ID", text: $settings.keys.traktID)
            TVField(label: "Trakt client secret", text: $settings.keys.traktSecret, secure: true)
            TVField(label: "Simkl client ID", text: $settings.keys.simklID)
            TVField(label: "OMDb API key (for Rotten Tomatoes scores)", text: $settings.keys.omdb)
            TVChip(label: "Save keys", selected: true) { settings.saveKeys() }
            if !settings.keysStatus.isEmpty { TVNote(text: settings.keysStatus) }
        }
    }
}

// MARK: Sync

private struct TVSyncSettings: View {
    let settings: SettingsModel
    let tracker: Tracker

    var body: some View {
        let status = tracker.status
        TVCard(title: "Your accounts") {
            TVNote(text: status.text, error: status.problem)
            ForEach(status.rows, id: \.service) { row in
                HStack(spacing: 20) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Sync with \(row.service.label)").font(Palette.font(26, .semibold)).foregroundStyle(Palette.text)
                        Text(row.detail).font(Palette.font(20)).foregroundStyle(Palette.textDim)
                    }
                    Spacer(minLength: 0)
                    if row.linked {
                        TVChip(label: row.on ? "On" : "Off", selected: row.on) { tracker.setSync(row.service, on: !row.on) }
                    } else {
                        TVChip(label: "Link") { settings.section = .accounts }
                    }
                }
                .focusSection()
            }
            if status.linked {
                TVChip(label: status.busy ? "Syncing…" : "Sync now") { tracker.syncNow() }.disabled(status.busy)
            }
            TVNote(text: Tracker.syncNote)
        }
        TVCard(title: "Backup") {
            TVNote(text: "\(Backup.whereWords): \(settings.backupStatus)")
            HStack(spacing: 20) {
                TVChip(label: settings.backupBusy ? "Working…" : "Back up now", selected: true) { settings.backUp() }
                    .disabled(settings.backupBusy)
                TVChip(label: "Restore") { settings.restore() }.disabled(settings.backupBusy)
            }
            TVNote(text: "Restore puts back anything missing on this Apple TV and removes nothing.")
        }
    }
}

// MARK: General

private struct TVGeneralSettings: View {
    let model: AppModel
    let settings: SettingsModel

    var body: some View {
        TVCard(title: "Appearance") {
            HStack(spacing: 20) {
                TVChip(label: "Dark", selected: model.dark) { model.setDark(true) }
                TVChip(label: "Light", selected: !model.dark) { model.setDark(false) }
            }
        }
        TVCard(title: "Where to watch") {
            TVNote(text: "A title's page lists the streaming services that have it in this country.")
            Picker("Country", selection: Binding(get: { settings.country }, set: { settings.setCountry($0) })) {
                ForEach(Array(settings.countries.enumerated()), id: \.offset) { index, name in
                    Text(name).tag(index)
                }
            }
            .pickerStyle(.menu)
        }
        TVCard(title: "Sound") {
            HStack(spacing: 20) {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Send surround sound to a receiver untouched").font(Palette.font(26, .semibold)).foregroundStyle(Palette.text)
                    Text("For an AV receiver or soundbar that decodes Dolby and DTS itself.")
                        .font(Palette.font(20)).foregroundStyle(Palette.textDim)
                }
                Spacer(minLength: 0)
                TVChip(label: settings.passthrough ? "On" : "Off", selected: settings.passthrough) {
                    settings.setPassthrough(!settings.passthrough)
                }
            }
        }
    }
}

// MARK: About

private struct TVAboutSettings: View {
    let settings: SettingsModel

    var body: some View {
        TVCard {
            HStack(spacing: 24) {
                Image("Halo").resizable().scaledToFit().frame(width: 90, height: 90)
                Text("Haven").font(Palette.heading(48)).foregroundStyle(Palette.text)
            }
            TVNote(text: "Version \(Build.version) (\(Build.buildNumber))")
            TVNote(text: "Something not working? Tell us on GitHub: github.com/TinusTK/Haven")
        }
        TVCard {
            Text(Self.credits)
                .font(Palette.font(22))
                .foregroundStyle(Palette.text)
                .fixedSize(horizontal: false, vertical: true)
                .focusable()
        }
    }

    static let credits = """
        Haven: a media player and tracker for the films and TV shows you love.

        Haven © 2026 TinusTK. Free open source software under the GNU GPL version 3.
        Video playback by mpv (libmpv, LGPL) and FFmpeg (LGPL), from MPVKit. Database by GRDB (MIT licence).

        Film and TV information from TMDB and Cinemeta. Tracking with Trakt and Simkl. Where to watch by JustWatch.
        This product uses the TMDB API but is not endorsed or certified by TMDB.
        """
}

#if ADDONS
// MARK: Add-ons (GitHub build)

private struct TVAddonSettings: View {
    @Bindable var settings: SettingsModel

    var body: some View {
        TVCard(title: "Install") {
            TVNote(text: "Type an add-on's address to install it. Its catalogues show up as shelves on Home, next to Cinemeta, the free catalogue of films and series built into Haven.")
            TVField(label: "https://…/manifest.json", text: $settings.addonAddress) { settings.installAddon() }
            TVChip(label: settings.addonBusy ? "Installing…" : "Install", selected: true) { settings.installAddon() }
                .disabled(settings.addonBusy)
            if let status = settings.addonStatus { TVNote(text: status.message, error: status.isError) }
        }
        TVCard(title: "Installed") {
            if settings.addons.isEmpty { TVNote(text: "No add-ons installed yet.") }
            ForEach(settings.addons, id: \.url) { row in
                HStack(spacing: 20) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(row.name).font(Palette.font(26, .semibold)).foregroundStyle(Palette.text)
                        Text([row.version.isEmpty ? "" : "Version \(row.version)", row.description]
                            .filter { !$0.isEmpty }.joined(separator: " · "))
                            .font(Palette.font(20)).foregroundStyle(Palette.textDim).lineLimit(2)
                    }
                    Spacer(minLength: 0)
                    TVChip(label: row.enabled ? "On" : "Off", selected: row.enabled) { settings.setAddon(row.url, on: !row.enabled) }
                    TVRemove(name: row.name, what: "Its catalogues leave Home.") { settings.removeAddon(row.url) }
                }
                .focusSection()
            }
        }
    }
}

/// Signing in to a Stremio account brings in the add-ons it uses.
private struct TVStremioCard: View {
    @Bindable var settings: SettingsModel

    var body: some View {
        TVCard(title: "Stremio") {
            if settings.stremioEmail.isEmpty {
                TVNote(text: "Sign in with your Stremio account and Haven uses the add-ons it has, asking them itself as Stremio does. Your password is not kept.")
                TVField(label: "Email address", text: $settings.stremioEmailEntered)
                TVField(label: "Password", text: $settings.stremioPasswordEntered, secure: true) { settings.stremioSignIn() }
                TVChip(label: settings.stremioBusy ? "Signing in…" : "Sign in", selected: true) { settings.stremioSignIn() }
                    .disabled(settings.stremioBusy)
            } else {
                TVNote(text: "Signed in as \(settings.stremioEmail). The add-ons on this account are used, and follow it each time Haven starts.")
                TVChip(label: "Sign out") { settings.stremioSignOut() }
            }
            if !settings.stremioStatus.isEmpty { TVNote(text: settings.stremioStatus) }
        }
    }
}
#endif
