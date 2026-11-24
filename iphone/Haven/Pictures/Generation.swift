import Foundation

/// Drops answers that come too late: each board or page (Home's shelves,
/// Search, a title's page, Explore…) counts how often it was started
/// afresh, and an answer is used only if it belongs to the latest start.
/// So a slow answer for the title opened before never lands on the one
/// open now.
@MainActor
final class Generation {
    private(set) var current: UInt64 = 0

    /// Starts afresh: answers to anything earlier no longer count.
    @discardableResult
    func next() -> UInt64 {
        current &+= 1
        return current
    }

    /// Whether an answer started at `token` still counts.
    func isCurrent(_ token: UInt64) -> Bool { token == current }
}
