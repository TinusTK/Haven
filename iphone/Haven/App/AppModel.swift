import Foundation
import Observation
import SwiftUI

/// Haven's pages, numbered as the Rust build saved them (`last_page`).
enum Page: Int, CaseIterable, Identifiable, Sendable {
    case home = 0
    case explore = 1
    case library = 2
    case liveTV = 3
    case calendar = 4
    case settings = 5

    var id: Int { rawValue }

    var label: String {
        switch self {
        case .home: "Home"
        case .explore: "Explore"
        case .library: "Library"
        case .liveTV: "Live TV"
        case .calendar: "Calendar"
        case .settings: "Settings"
        }
    }

    var icon: String {
        switch self {
        case .home: "home"
        case .explore: "explore"
        case .library: "library"
        case .liveTV: "livetv"
        case .calendar: "calendar"
        case .settings: "settings"
        }
    }
}

/// The whole app's state while it runs: which page shows, the theme, the
/// loading screen, What's New, and the parts that work behind the pages.
@MainActor @Observable
final class AppModel {
    let core: Core
    var store: Store { core.store }

    private(set) var settings: Settings
    /// The page showing. A title's page (S12) lies over it.
    private(set) var page: Page
    /// A title's page is open over the pages (no tab is then highlighted).
    var detailOpen = false
    /// Settings was tapped again while showing: back to its list (S17).
    private(set) var settingsBackToList = 0

    let tracker: Tracker
    let library: Library
    /// Live TV and radio.
    let live: LiveTVModel
    /// The Calendar page's month or week and day picked.
    let calendar: CalendarModel
    /// Haven's own player (libmpv).
    let player: PlayerModel
    /// A film or episode playing over AirPlay in Apple's player.
    private(set) var airPlay: AirPlaySession?
    let notes = WhatsNew.builtIn
    /// The Library's part showing: 0 Local, 1 Downloads, 2 Watchlist, 3 History.
    var libraryTab = 0
    /// The title whose page is open.
    private(set) var opening: Opening?

    // The loading screen.
    private(set) var splashProgress: Double = 0
    private(set) var splashShown = true
    private(set) var splashGone = false
    private var whatsNewDue = false
    // What's New.
    private(set) var whatsNewShown = false

    /// How far start-up has got, 0 to 1 (Home's shelves, the banner and the
    /// calendar feed it from S10 and S16; until then there is nothing to wait for).
    var startupProgress: () -> Double = { 1 }

    init(core: Core) {
        self.core = core
        let settings = (try? Settings.load(core.store)) ?? Settings()
        self.settings = settings
        page = Page(rawValue: min(max(settings.lastPage, 0), Page.allCases.count - 1)) ?? .home
        tracker = Tracker(store: core.store, keys: core.secrets)
        tracker.openLink = { Browser.open($0) }
        library = Library(store: core.store, keys: core.secrets)
        tracker.watchlistChanged = { [library] in library.reloadWatchlist() }
        // Marks brought in from the accounts.
        tracker.watchedChanged = { [library] in
            library.marked()
            library.title.refreshTicks()
        }
        library.queueSync = { [tracker] in tracker.queue($0, kind: $1, titleID: $2, videoID: $3) }
        player = PlayerModel(store: core.store)
        live = LiveTVModel(store: core.store)
        calendar = CalendarModel(library: library)
        player.closed = { [library] in library.playerClosed($0) }
        player.next = { [library] in library.playNext($0) }
        library.dismissPlayer = { [player] in player.dismiss() }
        library.play = { [weak self] in self?.play($0) }
        live.play = { [weak self] in self?.play($0) }
        library.open = { [weak self] in self?.open($0) }
        whatsNewDue = WhatsNew.due(notes, store: core.store, existing: core.existing)
    }

    /// Starts what works behind the pages, in the Rust build's order as the
    /// parts arrive (S10 on: shelves, Continue, Watchlist, calendar…).
    func start() {
        library.homeShowing(page == .home)
        library.start()
        startupProgress = { [weak self] in
            guard let self else { return 1 }
            return library.startupProgress(homeShowing: page == .home && !detailOpen)
        }
        tracker.start()
        live.start()
        runSplash()
    }

    /// Opens a title's page.
    func open(_ title: Opening) {
        opening = title
        detailOpen = true
        library.homeShowing(false)
        library.title.open(title)
    }

    func closeTitle() {
        library.title.close()
        opening = nil
        detailOpen = false
        library.homeShowing(page == .home)
    }

    // MARK: Playing

    /// Plays a film or episode: in the player app chosen in Settings, over
    /// AirPlay when an AirPlay device is chosen, else in Haven's player.
    func play(_ request: PlayRequest) {
        let page = library.title!
        page.playNote = ""
        let fromTitle = !request.titleID.isEmpty
        let other = PlayerRules.OtherPlayer.load(store)
        if PlayerRules.goesElsewhere(other, address: request.address) {
            // Another app does not say how far it got: no resuming with it.
            guard let link = URL(string: PlayerRules.otherPlayerLink(kind: other.kind, address: request.address)) else { return }
            UIApplication.shared.open(link) { opened in
                Task { @MainActor in
                    guard fromTitle else { return }
                    page.playNote = opened ? "Opened in your player."
                        : "Could not open your player: install \(PlayerRules.OtherPlayer.names[min(max(other.kind, 1), 3)]), or pick another in Settings."
                }
            }
            return
        }
        guard AirPlay.isChosen else { return player.play(request) }
        Task {
            if await AirPlay.canSend(request.address) {
                startAirPlay(request)
            } else {
                // The sound still goes to the AirPlay device.
                if fromTitle { page.playNote = AirPlay.cannotSend }
                player.play(request)
            }
        }
    }

    private func startAirPlay(_ request: PlayRequest) {
        airPlay?.stop()
        var start: Double?
        if let saved = try? store.progress(kind: request.kind, titleID: request.titleID, videoID: request.videoID) {
            start = Resume.from(position: saved.position, duration: saved.duration, finished: saved.finished)
        }
        let session = AirPlaySession(.init(address: request.address, title: request.title, detail: request.detail, start: start))
        let store = store
        session.progress = { position, duration in
            PlayerModel.remember(request, position: position, duration: duration, store: store)
        }
        session.finished = { [weak self, weak session] in
            guard let self, let session else { return }
            let played = PlayerModel.played(request, position: session.position, duration: session.duration, failed: false)
            library.playerClosed(played)
            // A failure stays on the bar a moment, so it can be read.
            let wait: Duration = session.failure == nil ? .zero : .seconds(6)
            Task {
                try? await Task.sleep(for: wait)
                if self.airPlay === session { self.airPlay = nil }
            }
        }
        airPlay = session
        session.start()
    }

    /// The AirPlay bar's stop button.
    func stopAirPlay() {
        airPlay?.stop()
    }

    /// Opens the Library on one of its parts (Home's row arrows).
    func openLibrary(_ tab: Int) {
        libraryTab = tab
        select(.library)
    }

    /// Home came into view or left it.
    func homeAppeared(_ shown: Bool) {
        library.homeShowing(shown && !detailOpen)
    }

    // MARK: Pages

    func select(_ new: Page) {
        if new == .settings && page == .settings && !detailOpen { settingsBackToList += 1 }
        if opening != nil { library.title.close() }
        opening = nil
        detailOpen = false
        page = new
        library.homeShowing(new == .home)
        settings.lastPage = new.rawValue
        save()
    }

    // MARK: Theme

    var dark: Bool { settings.theme == .dark }

    func setDark(_ dark: Bool) {
        settings.theme = dark ? .dark : .light
        save()
    }

    private func save() {
        do {
            try settings.save(store)
        } catch {
            Log.note("could not save the settings: \(error.localizedDescription)")
        }
    }

    // MARK: Loading screen

    /// The loading screen shows for at least a second (so it does not
    /// flash), else until start-up is done, and never more than 12 seconds,
    /// so a slow or dead network never keeps Haven shut. Then it fades, and
    /// What's New shows if it is due.
    nonisolated static let splashLongest: Double = 12
    static let splashTick: Double = 0.08
    static let splashFade: Double = 0.4

    /// The bar's place: in the first second it cannot run ahead of the clock.
    nonisolated static func splashTarget(real: Double, elapsed: Double) -> Double {
        let real = elapsed >= splashLongest ? 1 : min(max(real, 0), 1)
        return elapsed < 1 ? min(real, elapsed) : real
    }

    private func runSplash() {
        let started = Date()
        Task { [weak self] in
            while true {
                try? await Task.sleep(for: .seconds(Self.splashTick))
                guard let self else { return }
                let elapsed = Date().timeIntervalSince(started)
                splashProgress = max(splashProgress, Self.splashTarget(real: startupProgress(), elapsed: elapsed))
                if splashProgress >= 1 && elapsed >= 1 {
                    Log.note(String(format: "loading screen closed after %.1f s", elapsed))
                    withAnimation(.easeOut(duration: Self.splashFade)) { splashShown = false }
                    try? await Task.sleep(for: .seconds(Self.splashFade + 0.05))
                    splashGone = true
                    if whatsNewDue { whatsNewShown = true }
                    return
                }
            }
        }
    }

    // MARK: What's New

    func whatsNewDone(dontShowAgain: Bool) {
        WhatsNew.done(notes, store: store, dontShowAgain: dontShowAgain)
        whatsNewShown = false
    }
}

/// Opens a web address in the browser. Only http and https.
@MainActor
enum Browser {
    static func open(_ address: String) {
        guard let url = try? HTTP.webURL(address) else { return }
        UIApplication.shared.open(url)
    }
}
