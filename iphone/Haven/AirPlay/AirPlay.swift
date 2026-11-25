import AVFoundation
import Foundation

/// AirPlay: sending a film or episode to an Apple TV or AirPlay TV.
///
/// Haven's own player (libmpv) draws its own picture, which AirPlay cannot
/// send as video, so a title sent over AirPlay plays in Apple's player
/// (AVPlayer) instead. Apple's player takes MP4, MOV and HLS but not MKV,
/// AVI, WebM or raw MPEG-TS: those keep playing on the phone, and say so.
/// Sound alone (an AirPlay speaker) works with Haven's own player as is.
/// (User, 2026-11-25: AirPlay with Apple's player, no Chromecast.)
enum AirPlay {
    /// File endings Apple's player plays.
    static let sendable: Set<String> = ["mp4", "m4v", "mov", "m3u8", "mp3", "m4a", "aac"]
    /// File endings it does not.
    static let notSendable: Set<String> = ["mkv", "avi", "webm", "wmv", "flv", "ts", "m2ts", "mpg", "mpeg", "3gp", "divx", "ogv"]
    /// How long the check of an address without a file ending may take.
    static let probeWait: Double = 8

    static let cannotSend = "This one can't be sent over AirPlay. It plays on this iPhone."

    /// What the address says it is, before asking.
    enum Guess: Equatable, Sendable {
        case yes
        case no
        /// No file ending: ask Apple's player.
        case ask
    }

    /// Judges an address by its scheme and file ending.
    static func guess(_ address: String) -> Guess {
        guard let url = URL(string: address.trimmingCharacters(in: .whitespacesAndNewlines)),
              let scheme = url.scheme?.lowercased(), scheme == "http" || scheme == "https" else { return .no }
        let ending = url.pathExtension.lowercased()
        if sendable.contains(ending) { return .yes }
        if notSendable.contains(ending) { return .no }
        return .ask
    }

    /// Whether Apple's player can play the address (and so send it over
    /// AirPlay). An address without a file ending is asked about: Apple's
    /// player reads its first part to tell.
    static func canSend(_ address: String) async -> Bool {
        switch guess(address) {
        case .yes: return true
        case .no: return false
        case .ask:
            guard let url = URL(string: address) else { return false }
            let playable = await withTimeout(probeWait) { () -> Bool in
                (try? await AVURLAsset(url: url).load(.isPlayable)) ?? false
            }
            return playable ?? false
        }
    }

    /// The AirPlay device the sound and picture go to now, if any.
    static var device: String? {
        AVAudioSession.sharedInstance().currentRoute.outputs.first { $0.portType == .airPlay }?.portName
    }

    /// Whether an AirPlay device is chosen (in the AirPlay button or in
    /// Control Center): a title then goes there.
    static var isChosen: Bool { device != nil }

    /// Sets the phone up to play films: Haven's sound goes on with the
    /// screen locked and can go to AirPlay.
    static func preparePlayback() {
        do {
            try AVAudioSession.sharedInstance().setCategory(.playback, mode: .moviePlayback, policy: .longFormVideo)
            try AVAudioSession.sharedInstance().setActive(true)
        } catch {
            Log.note("could not set up playback sound: \(error.localizedDescription)")
        }
    }
}
