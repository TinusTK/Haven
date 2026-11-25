import Foundation
import SwiftUI
import Testing
@testable import Haven

/// The app shell: What's New (the Rust build's whats_new.rs test, ported),
/// the loading bar's pace, pages, the theme and Haven's icons.
struct WhatsNewTests {
    @Test func theBuiltInNotesRead() throws {
        let notes = try #require(WhatsNew.builtIn, "whats-new.json is in the app")
        #expect(!notes.version.isEmpty)
        #expect(!notes.sections.isEmpty)
        #expect(notes.sections.allSatisfy { !$0.title.isEmpty && !$0.lines.isEmpty })
        // The App Store build must never mention add-ons.
        let url = try #require(Bundle.main.url(forResource: "whats-new", withExtension: "json"))
        #expect(!(try String(contentsOf: url, encoding: .utf8)).lowercased().contains("add-on"))
    }

    @Test func notesShowAfterAnUpdateUntilPutAway() throws {
        let store = try Store.inMemory()
        let notes = WhatsNew.Notes(version: "9.9.9", sections: [.init(title: "New", lines: ["A thing."])])
        // A fresh install never sees them.
        #expect(!WhatsNew.due(notes, store: store, existing: false))
        #expect(!WhatsNew.due(notes, store: store, existing: true))
        // An update: shown at every start until "Don't show again".
        let newer = WhatsNew.Notes(version: "10.0.0", sections: notes.sections)
        #expect(WhatsNew.due(newer, store: store, existing: true))
        WhatsNew.done(newer, store: store, dontShowAgain: false)
        #expect(WhatsNew.due(newer, store: store, existing: true))
        WhatsNew.done(newer, store: store, dontShowAgain: true)
        #expect(!WhatsNew.due(newer, store: store, existing: true))
        #expect(try store.get(WhatsNew.seenKey, as: String.self) == "10.0.0")
        #expect(!WhatsNew.due(nil, store: store, existing: true))
    }
}

struct ShellTests {
    @Test func theLoadingBarNeverRunsAheadOfTheFirstSecond() {
        #expect(AppModel.splashTarget(real: 1, elapsed: 0.3) == 0.3)
        #expect(AppModel.splashTarget(real: 0.2, elapsed: 0.5) == 0.2)
        #expect(AppModel.splashTarget(real: 0.6, elapsed: 3) == 0.6)
        #expect(AppModel.splashTarget(real: 0.1, elapsed: 12) == 1, "never more than 12 seconds")
        #expect(AppModel.splashTarget(real: 7, elapsed: 2) == 1)
    }

    @Test func pagesKeepTheRustBuildsNumbers() {
        #expect(Page.allCases.map(\.rawValue) == [0, 1, 2, 3, 4, 5])
        #expect(Page.allCases.map(\.label) == ["Home", "Explore", "Library", "Live TV", "Calendar", "Settings"])
    }

    @MainActor
    @Test func theLastPageAndTheThemeAreKept() throws {
        let store = try Store.inMemory()
        try Settings(theme: .light, lastPage: 9).save(store)
        let core = Core(paths: try AppPaths.discover(), store: store, secrets: Secrets(), existing: true)
        let model = AppModel(core: core)
        #expect(model.page == .settings, "a page number past the end is the last page")
        #expect(!model.dark)
        model.select(.liveTV)
        model.setDark(true)
        #expect(try Settings.load(store) == Settings(theme: .dark, lastPage: 3))
        // Settings tapped again while showing: back to its list.
        model.select(.settings)
        let before = model.settingsBackToList
        model.select(.settings)
        #expect(model.settingsBackToList == before + 1)
    }

    @Test func everyIconHasADrawing() {
        for name in IconPaths.drawings.keys {
            #expect(!IconPaths.path(name).isEmpty, "\(name)")
            #expect(IconPaths.path(name).boundingRect.maxX <= 24.01 && IconPaths.path(name).boundingRect.minX >= -0.01, "\(name)")
        }
        #expect(IconPaths.path("no such icon").isEmpty)
    }

    @Test func arcsAndRelativeMovesAreRead() {
        // A whole circle drawn as two half arcs: 19 across, centred at 12, 12.
        let circle = SVGPath.read("M12 2.5 a9.5 9.5 0 1 0 0 19 a9.5 9.5 0 1 0 0 -19 Z").boundingRect
        #expect(abs(circle.minX - 2.5) < 0.01 && abs(circle.maxX - 21.5) < 0.01)
        #expect(abs(circle.minY - 2.5) < 0.01 && abs(circle.maxY - 21.5) < 0.01)
        let lines = SVGPath.read("M3 10.5 L12 3 L21 10.5").boundingRect
        #expect(lines == CGRect(x: 3, y: 3, width: 18, height: 7.5))
        #expect(SVGPath.tokenize("a1 1 0 0 1 1 -1") == [.command("a"), .number(1), .number(1), .number(0), .number(0), .number(1), .number(1), .number(-1)])
    }
}
