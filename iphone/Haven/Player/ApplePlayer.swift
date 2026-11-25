import Foundation

/// Settings › Player › "Use Apple's player": films and episodes Apple's
/// player can play open in it instead of Haven's own, for picture in
/// picture (user, 2026-11-25: Haven's player stays the default). Kept on
/// this device only (not in backups).
enum ApplePlayerSetting {
    static let key = "apple_player"

    static func isOn(_ store: Store) -> Bool {
        ((try? store.get(key, as: Bool.self)) ?? nil) ?? false
    }

    static func set(_ on: Bool, store: Store) throws {
        try store.put(key, on)
    }

    /// Said on the title page when a video goes to Haven's player instead.
    static let cannotPlay = "Apple's player cannot play this kind of video, so it plays in Haven's player (no picture in picture)."
}

#if os(iOS)
import AVKit
import UIKit

/// Plays one session full screen in Apple's player, with picture in
/// picture: swiping home or tapping the picture-in-picture button shrinks
/// it to a floating window and Haven (or any app) can be used; the window's
/// button brings the player back. Closing the player, or the window, stops
/// it and saves the place.
@MainActor
final class ApplePlayer: NSObject, AVPlayerViewControllerDelegate {
    private var controller: HavenPlayerController?
    private var session: AirPlaySession?
    private var inPictureInPicture = false
    private var restoring = false

    func play(_ session: AirPlaySession) {
        stop()
        let controller = HavenPlayerController()
        controller.player = session.player
        controller.allowsPictureInPicturePlayback = true
        controller.canStartPictureInPictureAutomaticallyFromInline = true
        controller.updatesNowPlayingInfoCenter = true
        controller.delegate = self
        controller.modalPresentationStyle = .fullScreen
        controller.gone = { [weak self, weak controller] in
            guard let self, controller === self.controller else { return }
            // Closed by the person (not shrunk to picture in picture).
            if !self.inPictureInPicture { self.stop() }
        }
        self.controller = controller
        self.session = session
        session.start()
        Self.top()?.present(controller, animated: true)
    }

    /// Stops what plays (the place is saved by the session).
    func stop() {
        let session = self.session, controller = self.controller
        self.session = nil
        self.controller = nil
        inPictureInPicture = false
        session?.stop()
        if let controller, controller.presentingViewController != nil {
            controller.dismiss(animated: true)
        }
    }

    // MARK: AVPlayerViewControllerDelegate

    nonisolated func playerViewControllerShouldAutomaticallyDismissAtPictureInPictureStart(_: AVPlayerViewController) -> Bool {
        true
    }

    nonisolated func playerViewControllerWillStartPictureInPicture(_: AVPlayerViewController) {
        MainActor.assumeIsolated { inPictureInPicture = true }
    }

    nonisolated func playerViewController(
        _ playerViewController: AVPlayerViewController,
        restoreUserInterfaceForPictureInPictureStopWithCompletionHandler completionHandler: @escaping (Bool) -> Void
    ) {
        // Called on the main thread, as every player screen call is.
        nonisolated(unsafe) let done = completionHandler
        nonisolated(unsafe) let asking = playerViewController
        MainActor.assumeIsolated {
            guard let controller, controller === asking, controller.presentingViewController == nil,
                  let top = Self.top()
            else { return done(true) }
            restoring = true
            top.present(controller, animated: true) { done(true) }
        }
    }

    nonisolated func playerViewControllerDidStopPictureInPicture(_: AVPlayerViewController) {
        MainActor.assumeIsolated {
            inPictureInPicture = false
            // The floating window was closed (not brought back): stop.
            if !restoring { stop() }
            restoring = false
        }
    }

    /// The screen on top, to show the player over.
    private static func top() -> UIViewController? {
        let window = UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .flatMap(\.windows)
            .first { $0.isKeyWindow }
        var top = window?.rootViewController
        while let shown = top?.presentedViewController { top = shown }
        return top
    }
}

/// Apple's player screen, telling when it is closed.
final class HavenPlayerController: AVPlayerViewController {
    var gone: () -> Void = {}

    override func viewDidDisappear(_ animated: Bool) {
        super.viewDidDisappear(animated)
        if isBeingDismissed || presentingViewController == nil { gone() }
    }
}
#endif
