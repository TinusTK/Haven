import Foundation

/// One round of keeping Haven and the linked accounts in step:
/// `exchange` (may take a while) renews tokens about to run out, sends the
/// queued changes, and with `pull` brings in each account's Watchlist and
/// what it says was watched. `changesSince` compares that with what the
/// account held at its last pull (`Seen`): what was added there comes in,
/// and what was taken off there (off the Watchlist, or marked unwatched)
/// goes here too. Only what changed on the account counts, so a title one
/// account still lists does not come back after it was taken off here. As
/// the Rust build (`haven-tracking/src/sync.rs`), same kv `sync_seen`.
enum Sync {
    /// What one account held at its last pull. Keys from `listedKey` and
    /// `watchedKey`.
    struct Seen: Codable, Equatable, Sendable {
        var watchlist: Set<String> = []
        var watched: Set<String> = []

        init(watchlist: Set<String> = [], watched: Set<String> = []) {
            self.watchlist = watchlist
            self.watched = watched
        }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            watchlist = try c.decodeIfPresent(Set<String>.self, forKey: .watchlist) ?? []
            watched = try c.decodeIfPresent(Set<String>.self, forKey: .watched) ?? []
        }

        /// Every account's, by the service's name (kv `sync_seen`).
        static let key = "sync_seen"
    }

    /// "movie|tt1" or "series|tt1".
    static func listedKey(_ title: Listed) -> String { "\(title.series ? "series" : "movie")|\(title.imdb)" }

    /// "movie|tt1|tt1" for a film, "series|tt1|tt1:2:3" for an episode.
    static func watchedKey(_ entry: Watched) -> String {
        if let (season, episode) = entry.episode { return "series|\(entry.imdb)|\(entry.imdb):\(season):\(episode)" }
        return "movie|\(entry.imdb)|\(entry.imdb)"
    }

    /// The kind, title id and (for a watched key) video id in a key.
    static func keyParts(_ key: String) -> (kind: String, titleID: String, videoID: String) {
        let parts = key.split(separator: "|", maxSplits: 2, omittingEmptySubsequences: false).map(String.init)
        return (parts.first ?? "", parts.count > 1 ? parts[1] : "", parts.count > 2 ? parts[2] : "")
    }

    /// Whether a change of this device's for what `key` names is still
    /// waiting to be sent: then its word stands.
    static func waiting(_ queued: [Change], _ key: String) -> Bool {
        let (kind, title, video) = keyParts(key)
        return queued.contains {
            $0.kind == kind && $0.titleID == title && (video.isEmpty || $0.what.hasPrefix("watchlist") || $0.videoID == video)
        }
    }

    /// What one account holds now; nil where it could not be read.
    struct Pulled: Sendable {
        var service: TrackingService
        var watchlist: [Listed]?
        var watched: [Watched]?
    }

    /// What changed on one account since its last pull, and its new `Seen`.
    struct AccountChanges {
        var addedListed: [Listed] = []
        var goneListed: [String] = []
        var addedWatched: [Watched] = []
        var goneWatched: [String] = []
        var seen = Seen()
    }

    /// Compares what an account holds with what it held at its last pull
    /// (nil the first time: everything is new, nothing was taken off).
    static func changesSince(_ previous: Seen?, _ pulled: Pulled) -> AccountChanges {
        var changes = AccountChanges(seen: previous ?? Seen())
        if let listed = pulled.watchlist {
            let now = Set(listed.map(listedKey))
            changes.addedListed = listed.filter { previous?.watchlist.contains(listedKey($0)) != true }
            if let previous { changes.goneListed = previous.watchlist.subtracting(now).sorted() }
            changes.seen.watchlist = now
        }
        if let watched = pulled.watched {
            let now = Set(watched.map(watchedKey))
            changes.addedWatched = watched.filter { previous?.watched.contains(watchedKey($0)) != true }
            if let previous { changes.goneWatched = previous.watched.subtracting(now).sorted() }
            changes.seen.watched = now
        }
        return changes
    }

    /// What one round brought back.
    struct Outcome: Sendable {
        /// Queued changes every account now has (to take off the queue).
        var sent: [Int64] = []
        var problems: [String] = []
        /// Accounts whose tokens were renewed (to keep in place of the old).
        var renewed: [TrackedAccount] = []
        /// What each account holds, when brought in.
        var pulls: [Pulled] = []
        var pulled = false
    }

    /// Renews, sends `queued` to every account, and with `pull` brings in
    /// what the accounts hold.
    static func exchange(_ client: TrackingClient, accounts: [TrackedAccount], queued: [Change], pull: Bool, now: Int64) async -> Outcome {
        var outcome = Outcome(pulled: pull)
        func problem(_ account: TrackedAccount, _ error: Error) {
            outcome.problems.append("\(account.service.label): \(error.localizedDescription)")
        }
        // A token about to run out is renewed first.
        var ready: [TrackedAccount] = []
        for account in accounts {
            do {
                if let renewed = try await client.renewed(account, now: now) {
                    outcome.renewed.append(renewed)
                    ready.append(renewed)
                } else {
                    ready.append(account)
                }
            } catch {
                problem(account, error)
            }
        }
        // Each change goes to every account; it leaves the queue once all of
        // them have it (sending the same again does no harm).
        for change in queued {
            let marks = change.what == "watched" || change.what == "unwatched"
            guard let subject = Tracking.subject(kind: change.kind, titleID: change.titleID, videoID: marks ? change.videoID : nil) else {
                outcome.sent.append(change.id)
                continue
            }
            var all = true
            // A whole series marked watched or not: nothing the services take.
            let nothingToSend = marks && subject.series && subject.episode == nil
            for account in ready where !nothingToSend {
                do {
                    switch change.what {
                    case "watched": try await client.markWatched(account, subject)
                    case "unwatched": try await client.markUnwatched(account, subject)
                    case "watchlist_add": try await client.setWatchlist(account, subject, on: true)
                    default: try await client.setWatchlist(account, subject, on: false)
                    }
                } catch {
                    all = false
                    problem(account, error)
                }
            }
            if all && !ready.isEmpty { outcome.sent.append(change.id) }
        }
        if pull {
            for account in ready {
                var pulled = Pulled(service: account.service)
                do { pulled.watchlist = try await client.watchlist(account) } catch { problem(account, error) }
                do { pulled.watched = try await client.watched(account) } catch { problem(account, error) }
                outcome.pulls.append(pulled)
            }
        }
        return outcome
    }

    /// Keeps renewed accounts in place of the old ones.
    static func keepRenewed(_ accounts: inout [TrackedAccount], _ renewed: [TrackedAccount]) {
        for account in renewed {
            accounts.removeAll { $0.service == account.service }
            accounts.append(account)
        }
    }

    /// Puts titles from an account's Watchlist on the store's, if new there.
    /// `now` in seconds since 1970. Oldest first, so the list keeps the
    /// service's order.
    @discardableResult
    static func addListed(_ store: Store, _ listed: [Listed], now: Int64) -> Int {
        var added = 0
        for (place, title) in listed.reversed().enumerated() {
            let kind = title.series ? "series" : "movie"
            if (try? store.inWatchlist(kind: kind, titleID: title.imdb)) ?? true { continue }
            if (try? store.addToWatchlist(Saved(kind: kind, titleID: title.imdb, title: title.title, year: title.year,
                                                  poster: nil, addedAt: now * 1000 + Int64(place)))) != nil {
                added += 1
            }
        }
        return added
    }

    /// Records what an account says was watched, where the store has nothing yet.
    @discardableResult
    static func addWatched(_ store: Store, _ watched: [Watched]) -> Int {
        watched.filter { entry in
            let (kind, videoID, detail): (String, String, String)
            if let (season, episode) = entry.episode {
                (kind, videoID, detail) = ("series", "\(entry.imdb):\(season):\(episode)", "S\(season) E\(episode)")
            } else {
                (kind, videoID, detail) = ("movie", entry.imdb, "")
            }
            let progress = WatchProgress(kind: kind, titleID: entry.imdb, videoID: videoID, title: entry.title, detail: detail,
                                         poster: nil, position: 0, duration: 0, finished: true, updatedAt: max(entry.at, 0) * 1000)
            return (try? store.addWatchedElsewhere(progress)) ?? false
        }.count
    }

    // MARK: Options (kv `sync_options`)

    struct Options: Codable, Equatable, Sendable {
        static let key = "sync_options"

        var watched = true
        var watchlist = true
        /// Also bring in what the accounts hold.
        var pull = true
        /// When the queue was last sent in full, in seconds since 1970.
        var lastSync: Int64 = 0
        var lastPull: Int64 = 0
        /// Linked accounts whose sync is switched off (by service name).
        /// Everything else syncs: watched, progress and the Watchlist, both
        /// ways (one switch per account).
        var off: [String] = []

        enum CodingKeys: String, CodingKey {
            case watched, watchlist, pull, off
            case lastSync = "last_sync"
            case lastPull = "last_pull"
        }

        init() {}

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            lastSync = try c.decodeIfPresent(Int64.self, forKey: .lastSync) ?? 0
            lastPull = try c.decodeIfPresent(Int64.self, forKey: .lastPull) ?? 0
            off = try c.decodeIfPresent([String].self, forKey: .off) ?? []
            // The three old switches are gone: an account syncs everything or nothing.
        }

        static func load(_ store: Store) -> Options {
            (try? store.get(key, as: Options.self)) ?? Options()
        }

        func save(_ store: Store) {
            do {
                try store.put(Self.key, self)
            } catch {
                Log.note("could not save the sync options: \(error.localizedDescription)")
            }
        }
    }
}
