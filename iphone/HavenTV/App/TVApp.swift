import SwiftUI

/// Haven for Apple TV: the same foundations as the iPhone app (`Core`,
/// `AppModel`), with screens made for the Siri Remote.
@main
struct HavenTVApp: App {
    /// The opened foundations, or why they could not be opened.
    private let core: Result<Core, Error>

    init() {
        core = Result { try Core.open() }
        if case let .failure(error) = core {
            Log.note("could not start: \(error.localizedDescription)")
        }
    }

    var body: some Scene {
        WindowGroup {
            TVRootView(core: core)
        }
    }
}
