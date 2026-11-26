import Foundation
import Observation

/// Settings: what each part shows and does. The rules behind each part
/// (servers, Live TV sources, add-ons, keys, tracking, where to watch, the
/// player) were ported in their own sections; this joins them to the page.
@MainActor @Observable
final class SettingsModel {
    /// The parts of Settings, in the Rust build's numbering.
    enum Section: Int, CaseIterable, Identifiable {
        case general = 0, accounts = 1, servers = 2, addons = 3, liveTV = 4, storage = 5, about = 6, sync = 7

        var id: Int { rawValue }

        var label: String {
            switch self {
            case .general: "General"
            case .storage: "Downloads & storage"
            case .accounts: "Accounts"
            case .sync: "Sync options"
            case .servers: "Media servers"
            case .addons: Self.extrasLabel
            case .liveTV: "Live TV"
            case .about: "About"
            }
        }

        #if ADDONS
        static let extrasLabel = "Add-ons"
        #else
        static let extrasLabel = ""
        #endif

        var icon: String {
            switch self {
            case .general: "general"
            case .storage: "storage"
            case .accounts: "account"
            case .sync: "sync"
            case .servers: "server"
            case .addons: "addon"
            case .liveTV: "livetv"
            case .about: "info"
            }
        }

        /// The list, under its groups (Add-ons only in the GitHub build).
        static var groups: [(name: String, sections: [Section])] {
            [("General", [.general, .storage]),
             ("Connections", Build.hasAddons ? [.accounts, .sync, .servers, .addons, .liveTV] : [.accounts, .sync, .servers, .liveTV]),
             ("Haven", [.about])]
        }
    }

    @ObservationIgnored private let core: Core
    @ObservationIgnored private let library: Library
    @ObservationIgnored private let tracker: Tracker
    @ObservationIgnored private let live: LiveTVModel
    @ObservationIgnored private let player: PlayerModel
    var store: Store { core.store }

    /// The part open; nil shows the list.
    var section: Section?
    /// Goes up when something saved changes, so the page reads it again.
    private(set) var revision = 0

    init(core: Core, library: Library, tracker: Tracker, live: LiveTVModel, player: PlayerModel) {
        self.core = core
        self.library = library
        self.tracker = tracker
        self.live = live
        self.player = player
        keys = (try? Keys.load(core.store)) ?? Keys()
    }

    private func changed() { revision += 1 }

    /// Shows what changed underneath (brought in from another device).
    func refresh() { changed() }

    // MARK: General

    var countries: [String] { WhereToWatch.settingsList() }

    var country: Int {
        _ = revision
        return WhereToWatch.settingsIndex(store)
    }

    func setCountry(_ index: Int) {
        do { try WhereToWatch.choose(index, store: store) } catch { Log.note("could not save the country: \(error.localizedDescription)") }
        changed()
    }

    /// 0 Haven's own player, 1 VLC, 2 Infuse, 3 Outplayer.
    var otherPlayer: Int {
        _ = revision
        return PlayerRules.OtherPlayer.load(store).kind
    }

    func setOtherPlayer(_ kind: Int) {
        var choice = PlayerRules.OtherPlayer.load(store)
        choice.kind = min(max(kind, 0), 3)
        do { try store.put(PlayerRules.OtherPlayer.key, choice) } catch { Log.note("could not save that: \(error.localizedDescription)") }
        changed()
    }

    /// Apple's player instead of Haven's, for picture in picture.
    var applePlayer: Bool {
        _ = revision
        return ApplePlayerSetting.isOn(store)
    }

    func setApplePlayer(_ on: Bool) {
        do { try ApplePlayerSetting.set(on, store: store) } catch { Log.note("could not save that: \(error.localizedDescription)") }
        changed()
    }

    var passthrough: Bool { player.preferences.passthrough }

    func setPassthrough(_ on: Bool) { player.setPassthrough(on) }

    var loud: Bool { player.preferences.loud }

    func setLoud(_ on: Bool) { player.setLoud(on) }

    // MARK: Accounts: your own keys

    var keys: Keys
    private(set) var keysStatus = ""

    func saveKeys() {
        do {
            try keys.save(store)
        } catch {
            keysStatus = "Could not save the keys: \(error.localizedDescription)"
            return
        }
        let file = (try? Secrets.fromFile(core.paths.secretsFile)) ?? Secrets()
        let resolved = keys.resolved(file: file)
        library.setKeys(resolved)
        tracker.setKeys(resolved)
        keysStatus = "Keys saved."
    }

    // MARK: Media servers

    enum ServerKindChoice: Int, CaseIterable { case jellyfin = 0, emby, plex }
    var serverKind: ServerKindChoice = .jellyfin
    var serverAddress = ""
    var serverUser = ""
    var serverPassword = ""
    private(set) var serverStatus: ServerAccounts.Status?
    private(set) var signingIn = false
    /// The Plex code to approve, and the page to approve it on.
    private(set) var plexCode = ""
    private(set) var plexLink = ""
    @ObservationIgnored private var signInTask: Task<Void, Never>?

    var servers: [Server] { library.servers.all }

    func serverOn(_ server: Server) -> Bool { library.servers.enabled.contains { $0.id == server.id } }

    func serverState(_ server: Server) -> ServerAccounts.State { library.serverStates[server.id] ?? .unknown }

    func signIn() {
        signInTask?.cancel()
        signingIn = true
        let client = library.serverClient
        if serverKind == .plex {
            serverStatus = ServerAccounts.Status(text: ServerAccounts.askingPlex, isError: false)
            signInTask = Task { [weak self] in
                let outcome = await ServerAccounts.plexSignIn(client: client) { pin in
                    await MainActor.run {
                        self?.plexCode = pin.code
                        self?.plexLink = pin.link
                        self?.serverStatus = ServerAccounts.Status(text: ServerAccounts.approvePlex, isError: false)
                        Browser.open(pin.link)
                    }
                }
                guard let self, let outcome else { return }
                switch outcome {
                case let .servers(found): self.added(found)
                case let .failed(why): self.finishSignIn(ServerAccounts.failed(why))
                }
            }
            return
        }
        serverStatus = ServerAccounts.Status(text: ServerAccounts.signingIn, isError: false)
        let (kind, address, user, password): (ServerKind, String, String, String) =
            (serverKind == .emby ? .emby : .jellyfin, serverAddress, serverUser, serverPassword)
        signInTask = Task { [weak self] in
            let result = await ServerAccounts.signIn(kind, address: address, user: user, password: password, client: client)
            guard let self else { return }
            switch result {
            case let .success(found): self.added(found)
            case let .failure(error): self.finishSignIn(ServerAccounts.failed(error.localizedDescription))
            }
        }
    }

    func cancelSignIn() {
        signInTask?.cancel()
        finishSignIn(nil)
    }

    private func added(_ found: [Server]) {
        let status = ServerAccounts.added(found, store: store)
        if !status.isError {
            serverAddress = ""
            serverUser = ""
        }
        finishSignIn(status)
        library.loadServers()
    }

    private func finishSignIn(_ status: ServerAccounts.Status?) {
        signingIn = false
        serverPassword = ""
        plexCode = ""
        plexLink = ""
        serverStatus = status
    }

    func setServer(_ server: Server, on: Bool) {
        serverStatus = ServerAccounts.setEnabled(server.id, on: on, store: store)
        library.loadServers()
    }

    func removeServer(_ server: Server) {
        serverStatus = ServerAccounts.remove(server.id, store: store)
        library.loadServers()
    }

    /// Asks every server again whether it answers.
    func checkServers() {
        library.reloadServerShelves()
    }

    // MARK: Live TV sources

    var playlistAddress = ""
    var xtreamServer = ""
    var xtreamUser = ""
    var xtreamPassword = ""
    private(set) var tvStatus: LiveTV.Status?
    private(set) var tvBusy = false

    var tvSources: [LiveTV.SourceRow] {
        _ = revision
        _ = live.tv.channels.count + live.radio.channels.count
        return LiveTV.sourceRows(store)
    }

    /// An account's state: nil while not known (or a playlist, or off).
    func tvOnline(_ row: LiveTV.SourceRow) -> Bool? {
        row.isAccount && row.enabled ? live.online[row.url] : nil
    }

    func addPlaylist() {
        switch LiveTV.playlistAddress(playlistAddress) {
        case let .failure(status):
            tvStatus = status
        case let .success(url):
            tvBusy = true
            tvStatus = LiveTV.Status(text: LiveTV.fetchingPlaylist, isError: false, account: false)
            Task {
                let result = await LiveTV.fetch(url)
                finishAdding(url, account: nil, result)
            }
        }
    }

    func addXtream() {
        guard let account = XtreamAccount(server: xtreamServer, username: xtreamUser, password: xtreamPassword) else {
            tvStatus = LiveTV.Status(text: LiveTV.accountNeeds, isError: true, account: true)
            return
        }
        tvBusy = true
        tvStatus = LiveTV.Status(text: LiveTV.signingIn, isError: false, account: true)
        Task {
            let result = await LiveTV.fetch(account)
            finishAdding(account.sourceURL, account: account, result)
        }
    }

    private func finishAdding(_ url: String, account: XtreamAccount?, _ result: Result<String, Problem>) {
        tvBusy = false
        let fetched = LiveTV.fetched(url, account: account, adding: true, result, store: store)
        tvStatus = fetched.status
        if fetched.saved {
            if account == nil {
                playlistAddress = ""
            } else {
                xtreamServer = ""
                xtreamUser = ""
                xtreamPassword = ""
            }
            live.rebuild()
            Task { await live.checkAccounts() }
        }
        changed()
    }

    func setTVSource(_ url: String, on: Bool) {
        if let status = LiveTV.setEnabled(url, on: on, store: store) { tvStatus = status }
        live.rebuild()
        if on { live.refresh(url) }
        changed()
    }

    func removeTVSource(_ url: String) {
        tvStatus = LiveTV.remove(url, store: store)
        live.rebuild()
        changed()
    }

    // MARK: Add-ons (GitHub build)

    #if ADDONS
    var addonAddress = ""
    private(set) var addonStatus: InstalledAddons.Outcome?
    private(set) var addonBusy = false

    var addons: [InstalledAddons.Row] {
        _ = revision
        return library.addons.rows
    }

    func installAddon() {
        let address = addonAddress
        addonBusy = true
        let store = store
        Task {
            let outcome = await InstalledAddons.install(address, store: store)
            addonBusy = false
            addonStatus = outcome
            if case .installed = outcome { addonAddress = "" }
            addonsChanged()
        }
    }

    func setAddon(_ url: String, on: Bool) {
        try? InstalledAddons.setEnabled(url, on: on, store: store)
        addonsChanged()
    }

    func removeAddon(_ url: String) {
        try? store.removeAddon(url: url)
        addonsChanged()
    }

    private func addonsChanged() {
        library.loadSources()
        library.reloadHome()
        changed()
    }

    // MARK: Stremio account (GitHub build)

    var stremioEmailEntered = ""
    var stremioPasswordEntered = ""
    private(set) var stremioStatus = ""
    private(set) var stremioBusy = false

    /// The signed-in account's email; empty when signed out.
    var stremioEmail: String {
        _ = revision
        return Stremio.account(store)?.email ?? ""
    }

    func stremioSignIn() {
        let (email, password) = (stremioEmailEntered, stremioPasswordEntered)
        guard !email.trimmingCharacters(in: .whitespaces).isEmpty, !password.isEmpty else {
            stremioStatus = "Type your Stremio email address and password."
            return
        }
        stremioBusy = true
        stremioStatus = "Signing in to Stremio…"
        let store = store
        Task {
            do {
                let account = try await Stremio.login(email: email, password: password)
                let listed = try await Stremio.listed(account)
                try store.put(Stremio.accountKey, account)
                stremioPasswordEntered = ""
                stremioStatus = ""
                if Stremio.follow(listed, store: store) { addonsChanged() } else { changed() }
            } catch {
                stremioStatus = "Could not sign in: \(error.localizedDescription)."
            }
            stremioBusy = false
        }
    }

    func stremioSignOut() {
        guard let account = Stremio.account(store) else { return }
        try? store.remove(Stremio.accountKey)
        // What came from the account goes.
        if Stremio.follow([], store: store) { addonsChanged() } else { changed() }
        stremioStatus = "Signed out of Stremio."
        Task { await Stremio.logout(account) }
    }

    /// At start: the account's list again. Offline, what is installed stays.
    func stremioRefresh() {
        guard let account = Stremio.account(store) else { return }
        let store = store
        Task {
            do {
                let listed = try await Stremio.listed(account)
                if Stremio.follow(listed, store: store) { addonsChanged() }
            } catch Stremio.Failure.refused {
                stremioStatus = "Stremio asks you to sign in again."
            } catch {
                Log.note("stremio: kept the last list: \(error.localizedDescription)")
            }
        }
    }
    #endif

    // MARK: Storage

    var cacheText: String {
        _ = revision
        return Pictures.shared.usageText()
    }

    func clearCache() {
        Pictures.shared.clear()
        changed()
    }

    // MARK: Backup

    private(set) var backupStatus = ""
    private(set) var backupBusy = false
    @ObservationIgnored private var lastWritten = ""
    @ObservationIgnored private var backupPlace = ""
    @ObservationIgnored private var backupTimer: Task<Void, Never>?

    /// Backs up a little after start and then every 10 minutes, when
    /// something changed.
    func startBackups() {
        showBackup()
        backupTimer?.cancel()
        backupTimer = Task { [weak self] in
            try? await Task.sleep(for: .seconds(Backup.afterStart))
            while !Task.isCancelled {
                self?.backUp(quiet: true)
                try? await Task.sleep(for: .seconds(Backup.every))
            }
        }
    }

    private func showBackup(_ note: String = "") {
        let last = ((try? store.get(Backup.lastKey, as: Int64.self)) ?? nil) ?? 0
        if !note.isEmpty {
            backupStatus = note
        } else if last == 0 {
            backupStatus = Backup.firstNote
        } else if backupPlace.isEmpty {
            backupStatus = "Last backed up \(Backup.when(last))."
        } else {
            backupStatus = "Last backed up \(Backup.when(last)) to \(backupPlace)."
        }
    }

    /// Writes the backup if anything changed since the last one (or, not
    /// `quiet`, always).
    func backUp(quiet: Bool = false) {
        guard !backupBusy else { return }
        let backup: [String: Any]
        do {
            backup = try Backup.make(store, now: Int64(Date().timeIntervalSince1970))
        } catch {
            return showBackup("Could not read what to back up: \(error.localizedDescription)")
        }
        let contents = Backup.contents(backup)
        if quiet && contents == lastWritten { return }
        backupBusy = true
        if !quiet { showBackup("Backing up…") }
        guard let text = try? Backup.text(backup) else { return }
        Task {
            let written: Result<String, Problem> = await Task.detached {
                let (folder, place) = Backup.folder()
                do {
                    try Backup.write(text: text, to: folder.appendingPathComponent(Backup.fileName))
                    return .success(place)
                } catch {
                    return .failure(Problem(error.localizedDescription))
                }
            }.value
            backupBusy = false
            switch written {
            case let .success(place):
                lastWritten = contents
                backupPlace = place
                try? store.put(Backup.lastKey, Int64(Date().timeIntervalSince1970))
                showBackup()
            case let .failure(problem):
                showBackup("Could not back up: \(problem.words).")
            }
        }
    }

    /// Reads the backup and adds what is missing here.
    func restore() {
        guard !backupBusy else { return }
        backupBusy = true
        showBackup("Looking for the backup…")
        Task {
            let read: Result<String, Problem> = await Task.detached {
                let (folder, place) = Backup.folder()
                let url = folder.appendingPathComponent(Backup.fileName)
                // iCloud may hold it without having fetched it yet.
                try? FileManager.default.startDownloadingUbiquitousItem(at: url)
                guard let text = try? String(contentsOf: url, encoding: .utf8) else {
                    return .failure(Problem("There is no backup in \(place) yet"))
                }
                return .success(text)
            }.value
            backupBusy = false
            let backup: [String: Any]
            switch read {
            case let .failure(problem):
                return showBackup("\(problem.words).")
            case let .success(text):
                guard let parsed = try? Backup.read(text) else { return showBackup("That file is not a Haven backup.") }
                backup = parsed
            }
            do {
                let merged = try Backup.restore(backup, into: store)
                if merged.tracking { tracker.reloadAccounts() }
                library.loadServers()
                library.reloadWatchlist()
                library.marked()
                library.title.refreshTicks()
                #if ADDONS
                library.loadSources()
                library.reloadHome()
                #endif
                live.rebuild()
                keys = (try? Keys.load(store)) ?? keys
                changed()
                let count = merged.added
                showBackup("Restored the backup from \(Backup.when(Backup.madeAt(backup))): \(count) new \(count == 1 ? "thing" : "things"). New TV sources and settings show the next time Haven starts.")
            } catch {
                showBackup("Could not restore the backup: \(error.localizedDescription)")
            }
        }
    }

    // MARK: About

    var dataFolder: String { core.paths.dataDir.path }
    var logFile: String { core.paths.logFile.path }
    static let newIssue = "https://github.com/TinusTK/Haven/issues/new"
    static let source = "https://github.com/TinusTK/Haven"
}
