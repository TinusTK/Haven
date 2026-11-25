import AVKit
import Observation
import SwiftUI

/// Apple's AirPlay button: tapping it lists the Apple TVs, AirPlay TVs and
/// speakers nearby, TVs first. It goes where the Rust build had "Play on"
/// (the sources panel) and in the player.
struct AirPlayButton: UIViewRepresentable {
    var tint: Color = Color("TextDim")
    var active: Color = Color("Accent")

    func makeUIView(context: Context) -> AVRoutePickerView {
        let picker = AVRoutePickerView()
        picker.prioritizesVideoDevices = true
        picker.backgroundColor = .clear
        return picker
    }

    func updateUIView(_ picker: AVRoutePickerView, context: Context) {
        picker.tintColor = UIColor(tint)
        picker.activeTintColor = UIColor(active)
    }
}

/// Whether there is anywhere to AirPlay to: the button shows only then.
@MainActor @Observable
final class AirPlayDevices {
    private(set) var nearby = false
    private let detector = AVRouteDetector()
    private var watcher: NSObjectProtocol?

    init() {
        nearby = detector.multipleRoutesDetected
        watcher = NotificationCenter.default.addObserver(
            forName: .AVRouteDetectorMultipleRoutesDetectedDidChange, object: detector, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated {
                    guard let self else { return }
                    self.nearby = self.detector.multipleRoutesDetected
                }
            }
    }

    /// Looks for devices while something can be sent (it uses some battery,
    /// so only while a list of sources or the player is on screen).
    func look(_ on: Bool) {
        detector.isRouteDetectionEnabled = on
        nearby = detector.multipleRoutesDetected
    }
}
