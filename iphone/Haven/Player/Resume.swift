import Foundation

/// When a title is remembered, resumed, or counted as watched (the Rust
/// build's `haven-player` resume rules).
enum Resume {
    /// Nothing is remembered before this many seconds have been watched.
    static let keepAfter = 30.0
    /// From this share of the running time on, the title counts as watched.
    static let watchedFrom = 0.95
    /// Resuming starts this many seconds earlier, to pick the thread back up.
    static let rewind = 3.0

    /// How far someone got into a video.
    enum Watched: Equatable, Sendable {
        /// Barely started: not worth remembering.
        case started
        /// Somewhere in the middle: remember the place.
        case partway
        /// At or near the end.
        case finished
    }

    /// Judges a position; nil when the running time is unknown (a live
    /// stream), where there is no place to remember.
    static func judge(position: Double, duration: Double) -> Watched? {
        guard position.isFinite, duration.isFinite, duration > 0 else { return nil }
        if position >= duration * watchedFrom { return .finished }
        if position >= keepAfter { return .partway }
        return .started
    }

    /// Where to start a title left at `position`; nil to start from the
    /// beginning.
    static func from(position: Double, duration: Double, finished: Bool) -> Double? {
        guard !finished, judge(position: position, duration: duration) == .partway else { return nil }
        return max(position - rewind, 0)
    }

    /// The share watched, from 0 to 1, for a progress bar.
    static func share(position: Double, duration: Double) -> Double {
        guard position.isFinite, duration.isFinite, duration > 0 else { return 0 }
        return min(max(position / duration, 0), 1)
    }
}

/// A time as the player shows it: "4:05", or "1:02:03" from an hour on.
func clockText(_ seconds: Double) -> String {
    let total = seconds.isFinite ? Int(max(seconds, 0)) : 0
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60)
    return h > 0 ? String(format: "%d:%02d:%02d", h, m, s) : String(format: "%d:%02d", m, s)
}
