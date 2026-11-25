import AVFoundation
import Foundation
import Observation

/// One title playing over AirPlay in Apple's player, with the bar shown
/// above the tab bar on every page: its title, where it plays and how far
/// it got, −10 s / play-pause / +30 s / stop. A new session replaces the
/// old one. Unlike the Rust build's Chromecast casting, the place is kept:
/// `progress` is told how far it got, as Haven's own player does.
@MainActor @Observable
final class AirPlaySession {
    /// What to play.
    struct Request: Equatable, Sendable {
        var address: String
        var title: String
        /// "S1 E2 · Name"; empty for a film.
        var detail: String
        /// Where to start, in seconds (the resume point).
        var start: Double?
    }

    let request: Request
    private(set) var position: Double = 0
    private(set) var duration: Double = 0
    private(set) var paused = false
    private(set) var ended = false
    /// Why it stopped, in words, if it failed.
    private(set) var failure: String?
    /// The AirPlay device it plays on, once it is there.
    private(set) var device: String?

    /// Told how far it got (every few seconds, on pause and at the end).
    var progress: (_ position: Double, _ duration: Double) -> Void = { _, _ in }
    /// Told once it ends or is stopped.
    var finished: () -> Void = {}

    let player = AVPlayer()
    private var ticker: Any?
    private var observers: [NSObjectProtocol] = []
    private var lastSaved = Date.distantPast

    init(_ request: Request) {
        self.request = request
        player.allowsExternalPlayback = true
        player.usesExternalPlaybackWhileExternalScreenIsActive = true
    }

    /// Starts playing, from the resume point.
    func start() {
        guard let url = URL(string: request.address) else {
            fail("the address is not usable")
            return
        }
        AirPlay.preparePlayback()
        let item = AVPlayerItem(url: url)
        player.replaceCurrentItem(with: item)
        if let start = request.start, start > 0 {
            player.seek(to: CMTime(seconds: start, preferredTimescale: 600), toleranceBefore: .zero, toleranceAfter: .positiveInfinity)
        }
        player.play()
        ticker = player.addPeriodicTimeObserver(forInterval: CMTime(seconds: 1, preferredTimescale: 600), queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
        observers.append(NotificationCenter.default.addObserver(
            forName: AVPlayerItem.didPlayToEndTimeNotification, object: item, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.end() }
            })
        observers.append(NotificationCenter.default.addObserver(
            forName: AVPlayerItem.failedToPlayToEndTimeNotification, object: item, queue: .main) { [weak self] note in
                let why = (note.userInfo?[AVPlayerItemFailedToPlayToEndTimeErrorKey] as? Error)?.localizedDescription
                MainActor.assumeIsolated { self?.fail(why ?? "the stream stopped") }
            })
    }

    private func tick() {
        guard let item = player.currentItem else { return }
        if item.status == .failed {
            fail(item.error?.localizedDescription ?? "the stream could not be played")
            return
        }
        let now = player.currentTime().seconds
        if now.isFinite { position = now }
        let length = item.duration.seconds
        duration = length.isFinite ? length : 0
        paused = player.timeControlStatus == .paused
        device = player.isExternalPlaybackActive ? (AirPlay.device ?? device) : nil
        if Date().timeIntervalSince(lastSaved) >= 5, !paused { save() }
    }

    private func save() {
        lastSaved = Date()
        progress(position, duration)
    }

    func togglePause() {
        if player.timeControlStatus == .paused {
            player.play()
            paused = false
        } else {
            player.pause()
            paused = true
            save()
        }
    }

    /// Jumps `seconds` on or back (−10, +30), never before the start.
    func skip(_ seconds: Double) {
        let target = max(player.currentTime().seconds + seconds, 0)
        player.seek(to: CMTime(seconds: target, preferredTimescale: 600))
        position = target
    }

    func stop() {
        guard !ended else { return }
        save()
        tearDown()
        ended = true
        finished()
    }

    private func end() {
        position = duration > 0 ? duration : position
        stop()
    }

    private func fail(_ why: String) {
        failure = why
        Log.note("AirPlay stopped: \(why)")
        stop()
    }

    private func tearDown() {
        if let ticker { player.removeTimeObserver(ticker) }
        ticker = nil
        observers.forEach(NotificationCenter.default.removeObserver)
        observers.removeAll()
        player.pause()
        player.replaceCurrentItem(with: nil)
    }

    // MARK: The bar

    /// The bar's first line: "Title · S1 E2 · Name", or the title alone.
    var barTitle: String { Self.barTitle(request) }

    /// The bar's second line.
    var barStatus: String {
        Self.barStatus(position: position, duration: duration, device: device, failure: failure, started: duration > 0 || position > 0)
    }

    nonisolated static func barTitle(_ request: Request) -> String {
        request.detail.isEmpty ? request.title : "\(request.title) · \(request.detail)"
    }

    nonisolated static func barStatus(position: Double, duration: Double, device: String?, failure: String?, started: Bool) -> String {
        if let failure { return "AirPlay stopped: \(failure)." }
        guard started else { return device.map { "Connecting to \($0)…" } ?? "Starting…" }
        let time = duration > 0 ? "\(clockText(position)) / \(clockText(duration))" : clockText(position)
        return device.map { "On \($0) · \(time)" } ?? time
    }
}
