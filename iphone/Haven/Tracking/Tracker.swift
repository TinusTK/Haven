import Foundation
import Observation

/// Trakt and Simkl in the app: the linked accounts, linking with a code
/// (Settings › Accounts), and sync (Settings › Sync options).
///
/// Haven keeps everything on this device first and notes each change in a
/// queue in its database. The queue goes to the linked accounts in one go:
/// 20 s after a change (so several changes travel together), 8 s after
/// start, and on "Sync now". A change that cannot be sent stays queued.
/// What the accounts hold is brought in at start, on coming back to Haven
/// (when the last time was 10 minutes ago or more), a little after
/// something was watched, every half hour while Haven is open, and on
/// "Sync now". Both ways: what was added on an account comes in, and what
/// was taken off there (off the Watchlist, or marked unwatched) is taken
/// off here too (`Sync.Seen`). Nothing runs while Haven is closed.
@MainActor @Observable
final class Tracker {
    /// How long after a change the queue is sent, and after start the
    /// first sync runs (Home loads first).
    var afterAChange: Double = 20
    var afterStart: Double = 8
    /// How long after a video closes things are brought in.
    var afterWatching: Double = 5
    /// Coming back to Haven brings things in when the last time was this long ago.
    static let pullOnReturnSeconds: Int64 = 10 * 60
    /// Brings things in this often while Haven is open.
    static let pullWhileOpen: Double = 30 * 60

    private let store: Store
    private(set) var keys: Secrets
    private let http: HTTP
    /// Seconds since 1970 (a stand-in clock in tests).
    var now: () -> Int64 = { Int64(Date().timeIntervalSince1970) }
    /// Overrides for the services' addresses (tests).
    var traktAPI = TrackingClient.traktAPI
    var simklAPI = TrackingClient.simklAPI

    private(set) var accounts: [TrackedAccount]
    private(set) var running = false
    private var again: Bool?
    private(set) var lastProblem = ""
    private var pending: Task<Void, Never>?
    private var pendingPull = false

    // Linking (Settings › Accounts).
    private(set) var linking = false
    private(set) var code = ""
    private(set) var link = ""
    private(set) var linkStatus = ""
    private var linkTask: Task<Void, Never>?

    /// Told when a sync brought in Watchlist titles or watched marks.
    var watchlistChanged: () -> Void = {}
    var watchedChanged: () -> Void = {}
    /// After a full pull: catch up what the accounts lack.
    var pulledAll: () -> Void = {}
    /// Takes watched marks an account no longer has off this device, each
    /// (kind, IMDb id, video id), with media-server titles linked to them;
    /// says for each whether anything was taken off. The Library sets it.
    var forgetWatched: ([(kind: String, titleID: String, videoID: String)]) -> [Bool]
    private var whileOpen: Task<Void, Never>?
    /// Opens a web page (the service's code page).
    var openLink: (String) -> Void = { _ in }

    init(store: Store, keys: Secrets, http: HTTP = .shared) {
        self.store = store
        self.keys = keys
        self.http = http
        accounts = TrackedAccount.load(store)
        forgetWatched = { gone in
            gone.map { item in
                let videos = item.kind == "series" ? [item.videoID] : ((try? store.finishedVideos(kind: item.kind, titleID: item.titleID)) ?? [])
                var any = false
                for video in videos where (try? store.progress(kind: item.kind, titleID: item.titleID, videoID: video))??.finished == true {
                    any = (try? store.removeVideoProgress(kind: item.kind, titleID: item.titleID, videoID: video)) != nil || any
                }
                return any
            }
        }
    }

    var client: TrackingClient {
        var client = TrackingClient(keys: keys, http: http)
        client.trakt = traktAPI
        client.simkl = simklAPI
        return client
    }

    func setKeys(_ keys: Secrets) {
        self.keys = keys
    }

    /// Reads the sign-ins again (a restored backup brought one).
    func reloadAccounts() {
        accounts = TrackedAccount.load(store)
    }

    func account(_ service: TrackingService) -> TrackedAccount? {
        accounts.first { $0.service == service }
    }

    // MARK: Sync

    var options: Sync.Options { Sync.Options.load(store) }

    /// The linked accounts whose sync is on.
    var syncing: [TrackedAccount] {
        let off = options.off
        return accounts.filter { !off.contains($0.service.label) }
    }

    /// The first run, a little after start, brings things in; so does one
    /// every half hour while Haven is open.
    func start() {
        schedule(after: afterStart, pull: true)
        whileOpen?.cancel()
        whileOpen = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(Self.pullWhileOpen))
                guard !Task.isCancelled, let self else { return }
                await self.run(pull: true)
            }
        }
    }

    /// Coming back to Haven: brings things in when the last time was a while ago.
    func cameBack() {
        if now() - options.lastPull >= Self.pullOnReturnSeconds { schedule(after: 1, pull: true) }
    }

    /// After a video: the watched mark goes out and what the accounts hold
    /// comes in, so another device's changes show here too.
    func watchedSomething() {
        if !syncing.isEmpty { schedule(after: afterWatching, pull: true) }
    }

    /// Runs the queue later. A run already waiting keeps its pull.
    private func schedule(after seconds: Double, pull: Bool) {
        pendingPull = pendingPull || pull
        pending?.cancel()
        pending = Task { [weak self] in
            try? await Task.sleep(for: .seconds(seconds))
            guard !Task.isCancelled, let self else { return }
            let pull = self.pendingPull
            self.pendingPull = false
            self.pending = nil
            await self.run(pull: pull)
        }
    }

    /// Notes a change to send to the linked accounts, and sends the queue a
    /// little later. Nothing linked, or a title the services cannot know
    /// (they go by IMDb id): nothing to send.
    func queue(_ what: String, kind: String, titleID: String, videoID: String) {
        guard !syncing.isEmpty, Tracking.subject(kind: kind, titleID: titleID, videoID: nil) != nil else { return }
        do {
            try store.queueChange(Change(what: what, kind: kind, titleID: titleID, videoID: videoID, createdAt: now()))
        } catch {
            Log.note("could not note a change to sync: \(error.localizedDescription)")
            return
        }
        schedule(after: afterAChange, pull: false)
    }

    /// Notes a Watchlist change for the linked accounts.
    func trackWatchlist(kind: String, titleID: String, on: Bool) {
        queue(on ? "watchlist_add" : "watchlist_remove", kind: kind, titleID: titleID, videoID: "")
    }

    /// Notes a finished film or episode for the linked accounts.
    func trackWatched(kind: String, titleID: String, videoID: String) {
        queue("watched", kind: kind, titleID: titleID, videoID: videoID)
    }

    /// Sends the queue, and with `pull` brings in what the accounts hold. A
    /// request while it runs is folded into one more run afterwards.
    func run(pull: Bool) async {
        if running {
            again = (again ?? false) || pull
            return
        }
        let accounts = syncing
        let queued = (try? store.queuedChanges()) ?? []
        if accounts.isEmpty || (queued.isEmpty && !pull) { return }
        running = true
        let outcome = await Sync.exchange(client, accounts: accounts, queued: queued, pull: pull, now: now())
        finished(outcome)
        if let pull = again {
            again = nil
            await run(pull: pull)
        }
    }

    /// "Sync now": sends the queue and brings things in.
    func syncNow() {
        Task { await run(pull: true) }
    }

    private func finished(_ outcome: Sync.Outcome) {
        if !outcome.renewed.isEmpty {
            Sync.keepRenewed(&accounts, outcome.renewed)
            TrackedAccount.save(accounts, store: store)
        }
        for id in outcome.sent { try? store.removeChange(id: id) }
        var options = options
        if outcome.problems.isEmpty { options.lastSync = now() }
        if outcome.pulled {
            let (listChanged, marksChanged) = takeIn(outcome.pulls)
            if listChanged { watchlistChanged() }
            if marksChanged { watchedChanged() }
            if outcome.problems.isEmpty { options.lastPull = now() }
        }
        options.save(store)
        lastProblem = outcome.problems.first ?? ""
        for problem in outcome.problems { Log.note("sync: \(problem)") }
        running = false
        if outcome.pulled && outcome.problems.isEmpty { pulledAll() }
    }

    /// Brings in what changed on each account since its last pull: what was
    /// added there, and what was taken off there goes here too, unless a
    /// change of this device's for it is still waiting to be sent. With
    /// more than one account, what changed on one is passed on to the
    /// others (not on an account's first pull, which would send its whole
    /// history). Says whether the Watchlist and the watched marks changed.
    private func takeIn(_ pulls: [Sync.Pulled]) -> (Bool, Bool) {
        var seen = ((try? store.get(Sync.Seen.key, as: [String: Sync.Seen].self)) ?? nil) ?? [:]
        let queued = (try? store.queuedChanges()) ?? []
        let others = syncing.count > 1
        var listChanged = false, marksChanged = false
        var passOn: [(String, String)] = []
        for pulled in pulls {
            let label = pulled.service.label
            let first = seen[label] == nil
            let changes = Sync.changesSince(seen[label], pulled)
            let before = passOn.count
            for title in changes.addedListed where Sync.addListed(store, [title], now: now()) > 0 {
                listChanged = true
                passOn.append(("watchlist_add", Sync.listedKey(title)))
            }
            for key in changes.goneListed where !Sync.waiting(queued, key) {
                let (kind, title, _) = Sync.keyParts(key)
                if (try? store.inWatchlist(kind: kind, titleID: title)) == true,
                   (try? store.removeFromWatchlist(kind: kind, titleID: title)) != nil {
                    listChanged = true
                    passOn.append(("watchlist_remove", key))
                }
            }
            for entry in changes.addedWatched where Sync.addWatched(store, [entry]) > 0 {
                marksChanged = true
                passOn.append(("watched", Sync.watchedKey(entry)))
            }
            let gone = changes.goneWatched.filter { !Sync.waiting(queued, $0) }
            for (key, removed) in zip(gone, forgetWatched(gone.map(Sync.keyParts))) where removed {
                marksChanged = true
                passOn.append(("unwatched", key))
            }
            if first { passOn.removeSubrange(before...) }
            seen[label] = changes.seen
        }
        do { try store.put(Sync.Seen.key, seen) } catch { Log.note("could not keep what the accounts hold: \(error.localizedDescription)") }
        if others {
            for (what, key) in passOn {
                let (kind, title, video) = Sync.keyParts(key)
                queue(what, kind: kind, titleID: title, videoID: video)
            }
        }
        return (listChanged, marksChanged)
    }

    /// Switches one account's sync on or off; on, it syncs at once.
    func setSync(_ service: TrackingService, on: Bool) {
        var options = options
        options.off.removeAll { $0 == service.label }
        if !on { options.off.append(service.label) }
        options.save(store)
        if on { Task { await run(pull: true) } }
    }

    /// What Settings › Sync options shows.
    struct Status: Equatable, Sendable {
        var text: String
        var waiting: Int
        var problem: Bool
        var busy: Bool
        var linked: Bool
        var rows: [Row]

        struct Row: Equatable, Sendable {
            var service: TrackingService
            var linked: Bool
            var on: Bool
            var detail: String
        }
    }

    static let syncNote = "Changes go out in one go shortly after they happen. "
        + "Haven brings in your accounts' Watchlist and what you watched when it opens, when you come back to it, "
        + "after you watch something, and every half hour while it is open. "
        + "What you take off or mark unwatched on one device goes on the others too. "
        + "Nothing runs while Haven is closed, so it barely touches the battery."

    var status: Status {
        let options = options
        let names = syncing.map(\.service.label).joined(separator: " and ")
        let waiting = syncing.isEmpty ? 0 : ((try? store.queuedChanges().count) ?? 0)
        let when: String
        if options.lastSync > 0 {
            let at = Date(timeIntervalSince1970: TimeInterval(options.lastSync))
            let parts = Calendar.current.dateComponents([.hour, .minute], from: at)
            let clock = String(format: "%02d:%02d", parts.hour ?? 0, parts.minute ?? 0)
            when = Calendar.current.isDateInToday(at)
                ? "Last synced today at \(clock)"
                : "Last synced \(at.formatted(.dateTime.day().month(.abbreviated))) at \(clock)"
        } else {
            when = "Not synced yet"
        }
        let text: String
        if syncing.isEmpty {
            text = "Link Trakt or Simkl (or switch one on below) and Haven keeps it up to date from this device."
        } else if running {
            text = "Syncing with \(names)…"
        } else if !lastProblem.isEmpty {
            text = "\(when). Last try: \(lastProblem). Haven tries again later."
        } else if waiting == 0 {
            text = "\(when). Everything is in step with \(names)."
        } else {
            text = "\(when). \(waiting) change\(waiting == 1 ? "" : "s") still to send to \(names)."
        }
        let rows = TrackingService.allCases.map { service in
            let linked = account(service) != nil
            let on = linked && !options.off.contains(service.label)
            return Status.Row(service: service, linked: linked, on: on,
                              detail: !linked ? "Not linked." : on ? "Watched, progress and Watchlist, kept in step both ways."
                                  : "Linked, but not syncing.")
        }
        return Status(text: text, waiting: waiting, problem: !lastProblem.isEmpty && !syncing.isEmpty,
                      busy: running, linked: !syncing.isEmpty, rows: rows)
    }

    // MARK: Linking

    /// What a service's row in Settings › Accounts says.
    func rowText(_ service: TrackingService) -> String {
        if account(service) != nil { return "Linked: what you finish is marked watched, and your watchlist is passed on." }
        if !client.hasKeys(service) {
            return "Not available in this copy of Haven yet (no \(service.label) app key)."
        }
        return service == .trakt
            ? "Not linked. Link opens trakt.tv and shows a short code to approve there."
            : "Not linked. Link opens simkl.com: sign in and allow Haven, and it links by itself."
    }

    /// Starts linking: shows the code, opens the service's page and waits
    /// for the user to approve it there.
    func connect(_ service: TrackingService) {
        linkTask?.cancel()
        linking = true
        linkStatus = "Asking \(service.label) for a code…"
        let client = client
        linkTask = Task { [weak self] in
            let result: Result<TrackedAccount, Error>
            do {
                let code = try await client.begin(service)
                guard let self, !Task.isCancelled else { return }
                self.code = code.userCode
                self.link = code.link
                self.linkStatus = service == .simkl
                    ? "Sign in on the Simkl page that just opened in your web browser and allow Haven. Haven links by itself."
                    : "Enter the code on the \(service.label) page that just opened in your web browser."
                self.openLink(code.link)
                result = await Self.wait(for: code, client: client, now: self.now)
            } catch {
                result = .failure(error)
            }
            guard let self, !Task.isCancelled else { return }
            self.connected(service, result)
        }
    }

    /// Asks every `interval` seconds until the code is approved or runs out.
    private static func wait(for code: SignInCode, client: TrackingClient, now: @escaping () -> Int64) async -> Result<TrackedAccount, Error> {
        let started = Date()
        while true {
            do {
                try await Task.sleep(for: .seconds(code.interval))
            } catch {
                return .failure(CancellationError())
            }
            if Date().timeIntervalSince(started) > Double(code.expiresIn) { return .failure(TrackingError.expired) }
            do {
                if let account = try await client.poll(code, now: await MainActor.run { now() }) { return .success(account) }
            } catch {
                return .failure(error)
            }
        }
    }

    func cancelLinking() {
        linkTask?.cancel()
        linkTask = nil
        idle()
        linkStatus = ""
    }

    private func idle() {
        linking = false
        code = ""
        link = ""
    }

    private func connected(_ service: TrackingService, _ result: Result<TrackedAccount, Error>) {
        idle()
        switch result {
        case let .success(account):
            accounts.removeAll { $0.service == service }
            accounts.append(account)
            TrackedAccount.save(accounts, store: store)
            linkStatus = "Linked to \(service.label)."
            // Linking turns its sync on: bring in what the new account
            // holds, and send what waits.
            setSync(service, on: true)
        case let .failure(error):
            linkStatus = "Could not link \(service.label): \(error.localizedDescription)."
        }
    }

    /// Unlinks: tells the service too (best effort) and forgets the sign-in.
    func disconnect(_ service: TrackingService) {
        if let account = account(service) {
            let client = client
            Task { try? await client.revoke(account) }
        }
        accounts.removeAll { $0.service == service }
        TrackedAccount.save(accounts, store: store)
        linkStatus = "Unlinked from \(service.label). Haven has forgotten the sign-in."
    }

    /// Copies a service's Watchlist into Haven's.
    func importWatchlist(_ service: TrackingService) async {
        guard let account = account(service) else { return }
        linkStatus = "Fetching your \(service.label) watchlist…"
        do {
            let listed = try await client.watchlist(account)
            let added = Sync.addListed(store, listed, now: now())
            watchlistChanged()
            linkStatus = "\(added) of \(listed.count) titles from \(service.label) were new and are now on your Watchlist."
        } catch {
            linkStatus = "Could not fetch your \(service.label) watchlist: \(error.localizedDescription)."
        }
    }
}
