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
    let notes = WhatsNew.builtIn
    /// The title asked to open (its page comes in S12; until then a stand-in shows).
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
        tracker.watchedChanged = { [library] in library.reloadContinue() }
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
        runSplash()
    }

    /// Opens a title's page.
    func open(_ title: Opening) {
        opening = title
        detailOpen = true
        library.homeShowing(false)
    }

    func closeTitle() {
        opening = nil
        detailOpen = false
        library.homeShowing(page == .home)
    }

    /// Home came into view or left it.
    func homeAppeared(_ shown: Bool) {
        library.homeShowing(shown && !detailOpen)
    }

    // MARK: Pages

    func select(_ new: Page) {
        if new == .settings && page == .settings && !detailOpen { settingsBackToList += 1 }
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
