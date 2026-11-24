import Foundation

/// One round of keeping Haven and the linked accounts in step:
/// `exchange` (may take a while) renews tokens about to run out, sends the
/// queued changes, and with `pull` brings in each account's Watchlist and
/// what it says was watched; `addListed` and `addWatched` then put what came
/// back into the store, where it is new there.
enum Sync {
    /// What one round brought back.
    struct Outcome: Sendable {
        /// Queued changes every account now has (to take off the queue).
        var sent: [Int64] = []
        var problems: [String] = []
        /// Accounts whose tokens were renewed (to keep in place of the old).
        var renewed: [TrackedAccount] = []
        var watchlists: [Listed] = []
        var watched: [Watched] = []
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
                do { outcome.watchlists += try await client.watchlist(account) } catch { problem(account, error) }
                do { outcome.watched += try await client.watched(account) } catch { problem(account, error) }
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
