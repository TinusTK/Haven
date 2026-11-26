import Foundation
import MediaPlayer
import Observation
import UIKit

/// How far a film or episode got, for the books (sync, the media server,
/// the next episode).
struct Played: Equatable, Sendable {
    var kind: String
    var titleID: String
    var videoID: String
    var position: Double
    var duration: Double
    var finished: Bool
    /// The media server item it played, if it came from one.
    var serverItem: String?
}

/// The player page: the video, its controls, and remembering how far each
/// film or episode was watched. The Rust build's `haven-app/src/player.rs`.
@MainActor @Observable
final class PlayerModel {
    /// Which list is open: none, audio, subtitles, speed, picture or info.
    /// `sound` is Apple TV's Sound boost.
    enum Menu: Int { case none = 0, audio, subtitles, speed, picture, info, sound }

    private struct Playing {
        var request: PlayRequest
        /// mpv has opened the stream.
        var loaded = false
        var failed = false
        var position: Double
        var duration: Double = 0
        var savedAt = Date()
    }

    @ObservationIgnored let store: Store
    @ObservationIgnored let mpv: MPV

    // What the page shows.
    private(set) var open = false
    private(set) var title = ""
    private(set) var subtitle = ""
    private(set) var paused = false
    /// Opening the stream, or waiting for more of it.
    private(set) var busy = false
    private(set) var error = ""
    private(set) var duration: Double = 0
    private(set) var position: Double = 0
    /// How far the stream has arrived, in seconds from the start.
    private(set) var buffered: Double = 0
    private(set) var elapsedText = ""
    private(set) var remainingText = ""
    private(set) var endsAt = ""
    /// "4.2 MB/s  ·  42% loaded"
    private(set) var net = ""
    private(set) var audioTracks: [Track] = []
    private(set) var subtitleTracks: [Track] = []
    /// A subtitle track is showing.
    private(set) var anySubtitle = false
    var menu: Menu = .none
    private(set) var preferences: PlayerRules.Preferences
    private(set) var speed = 1.0
    private(set) var fit: MPV.Fit = .whole
    private(set) var subtitleDelay = 0.0
    private(set) var info = ""
    /// The episode after this one, in words; empty if there is none.
    private(set) var nextTitle = ""
    /// Seconds until the next episode starts by itself; nil while not counting.
    private(set) var nextCountdown: Int?
    /// The controls are on screen.
    var controlsShown = true { didSet { placeSubtitles() } }

    /// Runs each time the page closes, with how far it got.
    @ObservationIgnored var closed: (Played?) -> Void = { _ in }
    /// The episode after the one playing is wanted; says whether it is coming.
    @ObservationIgnored var next: ((Played?) -> Bool)?

    @ObservationIgnored private var playing: Playing?
    @ObservationIgnored private var seekHeld: Date?
    @ObservationIgnored private var nextAt: Date?
    @ObservationIgnored private var tracksShown = ""
    @ObservationIgnored private var marginSet: Int?
    @ObservationIgnored private var ticker: Timer?
    @ObservationIgnored private var observers: [NSObjectProtocol] = []

    init(store: Store, mpv: MPV = .shared) {
        self.store = store
        self.mpv = mpv
        preferences = ((try? store.get(PlayerRules.Preferences.key, as: PlayerRules.Preferences.self)) ?? nil) ?? .init()
        let center = NotificationCenter.default
        // Leaving Haven: the place is kept, and the film waits (radio goes on).
        observers.append(center.addObserver(forName: UIApplication.didEnterBackgroundNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.leaving() }
        })
        // A radio station playing in the background answers the lock
        // screen's and headphones' play and pause.
        let commands = MPRemoteCommandCenter.shared()
        commands.togglePlayPauseCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.remote(nil) ?? .commandFailed }
        }
        commands.playCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.remote(true) ?? .commandFailed }
        }
        commands.pauseCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.remote(false) ?? .commandFailed }
        }
    }

    /// A radio station is playing: it goes on in the background (user,
    /// 2026-11-25); films and TV channels wait.
    var isRadio: Bool { playing?.request.kind == "radio" }

    /// The lock screen's play/pause (nil: toggle).
    private func remote(_ play: Bool?) -> MPRemoteCommandHandlerStatus {
        guard isRadio, mpv.status != nil else { return .noActionableNowPlayingItem }
        let now = !(play ?? paused)
        mpv.setPaused(now)
        paused = now
        showNowPlaying()
        return .success
    }

    /// What the lock screen and Control Center show for a radio station.
    private func showNowPlaying() {
        guard let request = playing?.request, isRadio else {
            MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
            return
        }
        var info: [String: Any] = [MPMediaItemPropertyTitle: request.title, MPNowPlayingInfoPropertyIsLiveStream: true,
                                   MPNowPlayingInfoPropertyPlaybackRate: paused ? 0.0 : 1.0]
        if !request.detail.isEmpty { info[MPMediaItemPropertyArtist] = request.detail }
        MPNowPlayingInfoCenter.default().nowPlayingInfo = info
    }

    // MARK: Opening and closing

    /// Opens the player page and plays the stream, from where it was left
    /// if it was left partway.
    func play(_ request: PlayRequest) {
        var at: Double?
        if request.remember, let saved = try? store.progress(kind: request.kind, titleID: request.titleID, videoID: request.videoID) {
            at = Resume.from(position: saved.position, duration: saved.duration, finished: saved.finished)
        }
        title = request.title
        subtitle = request.detail
        error = ""
        busy = true
        paused = false
        position = at ?? 0
        duration = 0
        buffered = 0
        elapsedText = ""
        remainingText = ""
        endsAt = ""
        net = ""
        info = ""
        nextTitle = request.next ?? ""
        nextCountdown = nil
        nextAt = nil
        // Each film or episode has its own subtitle timing.
        subtitleDelay = 0
        anySubtitle = false
        menu = .none
        audioTracks = []
        subtitleTracks = []
        tracksShown = ""
        seekHeld = nil
        controlsShown = true
        open = true
        playing = Playing(request: request, position: at ?? 0)
        AirPlay.preparePlayback()
        showNowPlaying()
        startTicking()
        if let broken = mpv.broken {
            fail("Haven's player cannot start on this iPhone. (\(broken))")
            return
        }
        mpv.play(request.address, at: at)
    }

    private func startTicking() {
        guard ticker == nil else { return }
        let timer = Timer(timeInterval: 0.25, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
        RunLoop.main.add(timer, forMode: .common)
        ticker = timer
    }

    private func stopTicking() {
        ticker?.invalidate()
        ticker = nil
    }

    private func fail(_ message: String) {
        guard playing != nil else { return }
        playing?.failed = true
        mpv.stop()
        busy = false
        error = message
    }

    /// Saves the place, stops and goes back to the page underneath.
    func close() {
        guard playing != nil else { return }
        save()
        nextAt = nil
        nextCountdown = nil
        let played = takePlayed()
        mpv.stop()
        open = false
        stopTicking()
        showNowPlaying()
        closed(played)
    }

    /// Leaves the page when nothing is playing (the next episode could not
    /// be started by itself).
    func dismiss() {
        guard playing == nil else { return }
        nextAt = nil
        nextCountdown = nil
        open = false
        stopTicking()
    }

    /// Haven went to the background: the place is kept and the film waits.
    func leaving() {
        guard playing != nil, !isRadio else { return }
        save()
        mpv.setPaused(true)
        paused = true
    }

    // MARK: Ticking

    /// Four times a second while the page is open: mpv's news, the
    /// position, the track menus, and saving the place now and then.
    func tick() {
        // The episode is over and the next one is counting down.
        if let nextAt {
            let left = nextAt.timeIntervalSinceNow
            nextCountdown = max(Int(left.rounded(.up)), 0)
            if left <= 0 { goNext() }
            return
        }
        guard let current = playing, !current.failed else { return }
        for event in mpv.poll() {
            switch event {
            case .loaded:
                playing?.loaded = true
                applySettings()
            case .finished:
                return finished()
            case let .failed(reason):
                // What was watched before it broke off still counts.
                save()
                return fail("This stream could not be played. (\(reason))")
            }
        }
        guard let status = mpv.status else { return }
        let loaded = playing?.loaded ?? false
        if loaded {
            playing?.position = status.position
            playing?.duration = status.duration
        }
        busy = !loaded || status.buffering
        guard loaded else { return }
        paused = status.paused
        duration = max(status.duration, 0)
        let left = max(status.duration - status.position, 0)
        net = PlayerRules.arrival(status.arriving, loaded: PlayerRules.shareLoaded(status))
        buffered = status.position + (status.ahead ?? 0)
        elapsedText = clockText(status.position)
        remainingText = status.duration > 0 ? "−\(clockText(left))" : "Live"
        endsAt = status.duration > 0 ? PlayerRules.endsAt(left: left, speed: speed) : ""
        if menu == .info { info = PlayerRules.info(mpv.details, source: current.request.source) }
        if seekHeld.map({ Date().timeIntervalSince($0) >= PlayerRules.seekHold }) ?? true {
            position = status.position
        }
        showTracks()
        placeSubtitles()
        if let savedAt = playing?.savedAt, Date().timeIntervalSince(savedAt) >= PlayerRules.saveEvery, !status.paused {
            save()
        }
    }

    private func showTracks() {
        let json = mpv.tracksJSON
        guard json != tracksShown else { return }
        tracksShown = json
        let tracks = PlayerRules.tracks(json)
        audioTracks = tracks.filter { $0.kind == .audio }
        subtitleTracks = tracks.filter { $0.kind == .subtitle }
        anySubtitle = subtitleTracks.contains(where: \.selected)
    }

    /// Keeps subtitles clear of the controls while those are on screen.
    private func placeSubtitles() {
        guard mpv.handle != nil else { return }
        let height = Double(UIScreen.main.bounds.height)
        let margin = controlsShown ? Int((PlayerRules.controlsHeight * 720 / max(height, 1)).rounded()) : PlayerRules.subtitleMargin
        if marginSet != margin {
            mpv.setSubtitleMargin(margin)
            marginSet = margin
        }
    }

    // MARK: Remembering

    /// Saves how far the film or episode got, if worth remembering.
    func save() {
        guard let current = playing else { return }
        playing?.savedAt = Date()
        Self.remember(current.request, position: current.position, duration: current.duration, store: store)
    }

    /// Keeps the place of a film or episode (Haven's player or AirPlay),
    /// if it is worth remembering.
    static func remember(_ request: PlayRequest, position: Double, duration: Double, store: Store) {
        guard request.remember else { return }
        let finished: Bool
        switch Resume.judge(position: position, duration: duration) {
        case .partway: finished = false
        case .finished: finished = true
        case .started, nil: return
        }
        do {
            try store.saveProgress(WatchProgress(kind: request.kind, titleID: request.titleID, videoID: request.videoID,
                                                 title: request.title, detail: request.detail, poster: request.poster,
                                                 position: position, duration: duration, finished: finished, updatedAt: nowMillis()))
        } catch {
            Log.note("could not save where this was left: \(error.localizedDescription)")
        }
    }

    /// How far it got, for the books; nil when barely started or not kept.
    static func played(_ request: PlayRequest, position: Double, duration: Double, failed: Bool) -> Played? {
        guard request.remember, !failed, let judged = Resume.judge(position: position, duration: duration), judged != .started else { return nil }
        return Played(kind: request.kind, titleID: request.titleID, videoID: request.videoID, position: position,
                      duration: duration, finished: judged == .finished, serverItem: request.serverItem)
    }

    /// How far what was playing got; takes it off the player.
    private func takePlayed() -> Played? {
        guard let current = playing else { return nil }
        playing = nil
        return Self.played(current.request, position: current.position, duration: current.duration, failed: current.failed)
    }

    /// Played to the end: an episode with another after it counts down to
    /// that one; anything else closes the page.
    private func finished() {
        guard let current = playing else { return }
        if current.duration > 0 { playing?.position = current.duration }
        guard current.request.next != nil, next != nil else { return close() }
        save()
        nextAt = Date().addingTimeInterval(PlayerRules.nextAfter)
        busy = false
        paused = true
        nextCountdown = Int(PlayerRules.nextAfter)
    }

    /// Moves on to the next episode without leaving the player; if there
    /// turns out to be none, the page closes.
    func goNext() {
        guard playing != nil else { return }
        nextAt = nil
        save()
        let played = takePlayed()
        mpv.stop()
        nextCountdown = nil
        menu = .none
        subtitle = nextTitle
        nextTitle = ""
        error = ""
        busy = true
        duration = 0
        position = 0
        elapsedText = ""
        remainingText = ""
        endsAt = ""
        let coming = next?(played) ?? false
        if !coming { dismiss() }
    }

    /// The next episode's countdown was stopped: the page closes.
    func cancelNext() {
        close()
    }

    // MARK: Controls

    func togglePause() {
        guard let status = mpv.status else { return }
        mpv.setPaused(!status.paused)
        paused = !status.paused
        showNowPlaying()
        // Pausing is a good moment to remember the place.
        if !status.paused { save() }
    }

    func seek(to seconds: Double) {
        mpv.seek(to: seconds)
        hold(seconds)
    }

    func skip(_ seconds: Double) {
        guard let status = mpv.status else { return }
        mpv.seek(by: seconds)
        var target = max(status.position + seconds, 0)
        if status.duration > 0 { target = min(target, status.duration) }
        hold(target)
    }

    private func hold(_ seconds: Double) {
        seekHeld = Date()
        position = seconds
    }

    func chooseAudio(_ id: Int) {
        mpv.selectAudio(id)
        menu = .none
    }

    /// Nil turns subtitles off.
    func chooseSubtitle(_ id: Int?) {
        mpv.selectSubtitle(id)
        menu = .none
    }

    func setVolume(_ volume: Double) {
        // Moving the volume turns the sound back on.
        change { $0.volume = min(max(volume, 0), $0.top); $0.muted = false }
    }

    /// Turned off, a volume above 100% comes back down to 100%.
    func setLoud(_ on: Bool) {
        change { $0.loud = on; $0.volume = min($0.volume, $0.top) }
    }

    func toggleMute() {
        change { $0.muted.toggle() }
    }

    func setPassthrough(_ on: Bool) {
        change { $0.passthrough = on }
    }

    func stepSubtitleScale(_ way: Int) {
        change { $0.subtitleScale = min(max(($0.subtitleScale + 0.1 * Double(way.signum())) * 10, 5), 25).rounded() / 10 }
    }

    /// Half a second later or earlier.
    func stepSubtitleDelay(_ way: Int) {
        subtitleDelay = min(max(subtitleDelay + 0.5 * Double(way.signum()), -60), 60)
        applySettings()
    }

    /// 1: the next line shows now; -1: the line before; 0: no delay.
    func syncSubtitles(_ way: Int) {
        let delay = way == 0 ? 0 : mpv.syncSubtitles(way)
        if let delay {
            subtitleDelay = min(max(delay, -60), 60)
            applySettings()
        }
    }

    func setSpeed(_ speed: Double) {
        self.speed = min(max(speed, 0.25), 4)
        applySettings()
        menu = .none
    }

    func setFit(_ fit: MPV.Fit) {
        self.fit = fit
        applySettings()
        menu = .none
    }

    func toggleMenu(_ wanted: Menu) {
        menu = menu == wanted ? .none : wanted
        if menu == .info, let current = playing { info = PlayerRules.info(mpv.details, source: current.request.source) }
    }

    private func change(_ edit: (inout PlayerRules.Preferences) -> Void) {
        edit(&preferences)
        do {
            try store.put(PlayerRules.Preferences.key, preferences)
        } catch {
            Log.note("could not save the player's settings: \(error.localizedDescription)")
        }
        applySettings()
    }

    /// Gives mpv the volume, speed and the like.
    private func applySettings() {
        mpv.setVolume(preferences.volume)
        mpv.setMuted(preferences.muted)
        mpv.setSubtitleScale(preferences.subtitleScale)
        mpv.setPassthrough(preferences.passthrough)
        mpv.setSpeed(speed)
        mpv.setFit(fit)
        mpv.setSubtitleDelay(subtitleDelay)
    }
}
