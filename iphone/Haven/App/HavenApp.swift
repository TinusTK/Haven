import SwiftUI

@main
struct HavenApp: App {
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
            RootView(core: core)
        }
    }
}
