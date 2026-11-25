import SwiftUI

@main
struct HavenApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
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

/// What iOS tells Haven outside its windows: downloads that finished while
/// Haven was in the background or closed.
final class AppDelegate: NSObject, UIApplicationDelegate {
    func application(_ application: UIApplication, handleEventsForBackgroundURLSession identifier: String,
                     completionHandler: @escaping () -> Void) {
        guard identifier == DownloadSession.identifier else { return completionHandler() }
        DownloadSession.shared.finishedEvents = completionHandler
    }
}
