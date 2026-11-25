import Foundation
import Observation

/// What opening a title needs (from a card, the banner, Upcoming…). The
/// title's page comes in S12.
struct Opening: Equatable, Sendable {
    var id: String
    var kind: String
    var title: String
    /// The episode to show (from Continue Watching or Upcoming).
    var video: String?
    var poster: String?
}

/// Haven's titles: where they come from (Cinemeta, add-ons, media servers,
/// TMDB), Home's shelves, the banner, Continue Watching, the Watchlist and
/// when its titles come out. The Rust build's `Library`.
@MainActor @Observable
final class Library {
    let store: Store
    private(set) var keys: Secrets
    let http: HTTP
    let catalogues: Catalogues
    let serverClient: ServerClient
    let kept: Kept
    /// Home's work waits while Home is not showing.
    let homePause = Pause()
    private let serverGate = Gate(ShelfLength.serversAtOnce)
    private let calendarGate = Gate(2)
    /// Seconds since 1970 and today (stand-ins in tests).
    var now: () -> Int64 = { Int64(Date().timeIntervalSince1970) }
    var zone: TimeZone = .current

    // MARK: Sources

    private(set) var builtin: Source
    #if ADDONS
    private(set) var addons = InstalledAddons()
    #endif
    /// Cinemeta first, then the switched-on add-ons.
    var installed: [Source] {
        #if ADDONS
        [builtin] + addons.sources
        #else
        [builtin]
        #endif
    }

    private(set) var servers = ServerAccounts.Loaded()
    /// Each switched-on server's libraries, as last listed.
    private(set) var serverLibraries: [(server: Server, library: MediaLibrary)] = []
    private(set) var serverStates: [Int64: ServerAccounts.State] = [:]

    /// TMDB, when a key is set.
    var tmdb: TMDB? { keys.tmdbAPIKey.map { TMDB(key: $0, http: http) } }

    // MARK: Boards

    let homeBoard = ShelfBoard(.home)
    let serverBoard = ShelfBoard(.servers)
    let catalogBoard = ShelfBoard(.catalog)
    let searchBoard = ShelfBoard(.search)
    /// Why search shows nothing (nothing added that can be searched).
    private(set) var searchNote = ""
    /// Explore's grid of one source at a time.
    @ObservationIgnored private(set) var explore: Explore!
    /// Home's requests still out (pull to refresh waits for them).
    private(set) var fetchesLeft = 0
    private(set) var refreshing = false

    // MARK: Home's top

    private(set) var hero: [HomeTop.Slide] = []
    private var heroStarted = false
    private var heroSettled = 0
    /// The banner's first picture has been shown (the loading screen waits for it).
    var heroFirstPicture = false
    private(set) var continueCards: [HomeTop.ContinueCard] = []
    private var continueKnown: [Releases.Title: (background: String?, videos: [Video])] = [:]
    private var continueAsked: Set<Releases.Title> = []
    private(set) var watchlist: [Saved] = []

    // MARK: When things come out

    private var releases: [Releases.Title: [Release]] = [:]
    private var calendarAsked: Set<Releases.Title> = []
    private(set) var calendarWaiting = 0
    private var calendarPictures: [Releases.Title: String] = [:]
    private(set) var upcoming: [Releases.Card] = []

    /// Opens a title's page.
    var open: (Opening) -> Void = { _ in }
    /// The open title's page.
    @ObservationIgnored private(set) var title: TitlePage!
    /// The Xtream Codes accounts' films and series, fetched once a run.
    let onDemand: XtreamOnDemand
    /// Plays a film or episode: Haven's player, AirPlay or another app.
    var play: ((PlayRequest) -> Void)?
    /// Leaves the player when the next episode could not start by itself.
    var dismissPlayer: () -> Void = {}
    /// Where the last film or episode played from (its source and the
    /// row's name): the next episode plays from the same place.
    @ObservationIgnored var lastPlayed: (source: String, name: String)?
    /// Notes a watched mark or Watchlist change for the linked accounts.
    var queueSync: (_ what: String, _ kind: String, _ titleID: String, _ videoID: String) -> Void = { _, _, _, _ in }
    /// Films and episodes saved to this iPhone.
    let downloads: Downloads
    /// Every title watched, the latest film or episode of each, newest first.
    private(set) var history: [WatchProgress] = []
    /// Videos in Files › On My iPhone › Haven › Videos.
    private(set) var localVideos: [DownloadRules.Found] = []
    private(set) var localStatus = ""
    private(set) var localBusy = false
    /// Goes up whenever something is marked watched or unwatched: views
    /// showing ticks read it, so they draw again.
    private(set) var marks = 0

    init(store: Store, keys: Secrets, http: HTTP = .shared, kept: Kept = Cache.kept) {
        self.store = store
        self.keys = keys
        self.http = http
        self.kept = kept
        catalogues = Catalogues(http: http)
        serverClient = ServerClient(device: DeviceID.get(store), http: http)
        builtin = Cinemeta.kept(store)
        onDemand = XtreamOnDemand(http: http)
        downloads = Downloads(store: store)
        explore = Explore(library: self)
        title = TitlePage(library: self)
    }

    // MARK: After playing

    /// The player closed: what was watched is told to the server and the
    /// linked accounts, and every tick and Continue Watching put right.
    func playerClosed(_ played: Played?) {
        if let played {
            report(played)
            if played.finished { queueSync("watched", played.kind, played.titleID, played.videoID) }
        }
        title.sources.autoplay = nil
        marked()
        title.refreshTicks()
    }

    /// The player wants the next episode: it is opened on the title's page
    /// and plays from the same place as the last as soon as that answers.
    /// False if there is no next episode.
    func playNext(_ played: Played?) -> Bool {
        if let played {
            report(played)
            if played.finished { queueSync("watched", played.kind, played.titleID, played.videoID) }
        }
        reloadContinue()
        guard let next = title.nextVideo, let last = lastPlayed else { return false }
        if let season = next.season, season != title.season { title.showSeason(season) }
        title.sources.autoplay = last
        title.openEpisode(next.id)
        // Nothing to ask: the list is already showing instead.
        return title.sources.autoplay != nil || title.sources.stillAsking
    }

    /// Tells the media server how far its film or episode was watched.
    func report(_ played: Played) {
        guard played.duration > 0,
              let (server, item) = ServerAccounts.serverItem(played.serverItem ?? played.videoID, in: servers.enabled) else { return }
        let client = serverClient
        Task {
            do {
                try await client.report(server, item: item, position: played.position, duration: played.duration, finished: played.finished)
            } catch {
                Log.note("could not tell \(server.name) what was watched: \(HTTP.words(error))")
            }
        }
    }

    /// Notes a Watchlist change for the linked accounts.
    func trackWatchlist(_ kind: String, _ titleID: String, _ on: Bool) {
        queueSync(on ? "watchlist_add" : "watchlist_remove", kind, titleID, "")
    }

    /// Something was marked watched or unwatched: Continue Watching, the
    /// Watchlist and every tick on screen are put right.
    func marked() {
        marks += 1
        reloadContinue()
        reloadWatchlist()
    }

    /// Loads what was kept and starts filling Home, in the Rust build's order.
    func start() {
        loadSources()
        Task { await refreshBuiltin() }
        reloadHome()
        reloadContinue()
        reloadWatchlist()
        loadServers()
        reloadCatalog()
        explore.rebuild()
        downloads.start()
        // The Videos folder shows in the Files app only once it exists.
        try? FileManager.default.createDirectory(at: DownloadRules.videosFolder, withIntermediateDirectories: true)
    }

    // MARK: Library › Local and History

    /// Looks for the videos in Haven › Videos (the Local tab opened, or Look again).
    func lookForVideos(in folder: URL = DownloadRules.videosFolder) {
        guard !localBusy else { return }
        localBusy = true
        localStatus = "Looking for videos…"
        Task {
            let found = await Task.detached { DownloadRules.findVideos(in: folder) }.value
            localVideos = found
            localBusy = false
            localStatus = DownloadRules.localStatus(found.count)
        }
    }

    /// Plays a video from Haven › Videos (always in Haven's own player:
    /// other apps cannot open files kept inside Haven).
    func playLocal(_ found: DownloadRules.Found) {
        let id = "local:\(found.path)"
        play?(PlayRequest(address: found.path, title: found.name, detail: "", kind: "movie", titleID: id, videoID: id,
                          poster: nil, source: "On this device"))
    }

    func reloadHistory() {
        history = (try? store.history(limit: 300)) ?? []
    }

    func setKeys(_ keys: Secrets) {
        self.keys = keys
        reloadCatalog()
        explore.rebuild()
    }

    /// Home came into view or left it: its loading goes on or waits.
    func homeShowing(_ showing: Bool) {
        homePause.set(shut: !showing)
    }

    // MARK: Sources

    func loadSources() {
        #if ADDONS
        addons = InstalledAddons.load(store)
        #endif
        explore?.rebuild()
    }

    /// Asks Cinemeta which catalogues it has now; Home is rebuilt if they changed.
    func refreshBuiltin() async {
        guard let fresh = await Cinemeta.refresh(store, current: builtin, catalogues: catalogues) else { return }
        builtin = fresh
        reloadHome()
        explore.rebuild()
    }

    // MARK: Shelves

    /// Rebuilds Home's catalogue shelves: one per catalogue that needs no input.
    func reloadHome() {
        let plans = installed.flatMap { source in
            source.catalogs.filter { !$0.needsInput }.map {
                ShelfPlan(title: $0.title, source: source.name, from: .catalogue(url: source.url, catalog: $0))
            }
        }
        fill(homeBoard, plans)
    }

    /// Rebuilds the TMDB shelves on Home; none without a TMDB key.
    func reloadCatalog() {
        let plans = tmdb == nil ? [] : TMDBShelves.plans.map {
            ShelfPlan(title: $0.title, source: "TMDB", from: .tmdb($0.list, series: $0.series))
        }
        fill(catalogBoard, plans)
    }

    /// Puts the planned shelves on a board, each loading, and fetches them.
    /// What a shelf held last time shows until the fresh list comes.
    func fill(_ board: ShelfBoard, _ plans: [ShelfPlan], search: String? = nil) {
        let generation = board.generation.next()
        var kept: [(Int, [MetaPreview])] = []
        board.shelves = plans.enumerated().map { index, plan in
            let name = board.board == .search ? "" : Kept.shelf(board: board.board.rawValue, title: plan.title, source: plan.source)
            if !name.isEmpty, let list = self.kept.read(name, as: [MetaPreview].self) { kept.append((index, list)) }
            return Shelf(id: index, title: plan.title, source: plan.source, keptName: name)
        }
        let counted = board.board != .search
        for (index, plan) in plans.enumerated() {
            if counted { fetchesLeft += 1 }
            let pause = counted ? homePause : nil
            Task {
                await pause?.pass()
                let result = await self.titles(plan.from, search: search)
                if counted { self.fetchesLeft = max(self.fetchesLeft - 1, 0) }
                self.loaded(board, generation: generation, shelf: index, result, fromKept: false)
            }
        }
        for (index, list) in kept {
            loaded(board, generation: generation, shelf: index, .success(list), fromKept: true)
        }
    }

    /// Fetches a shelf's titles.
    private func titles(_ from: ShelfFrom, search: String?) async -> Result<[MetaPreview], Problem> {
        do {
            switch from {
            case let .catalogue(url, catalog):
                return .success(try await catalogues.catalog(url, catalog, search: search))
            case let .server(server, library, latest):
                // A media server's shelves take turns.
                let client = serverClient
                return .success(try await serverGate.run {
                    latest ? try await client.latest(server, library: library)
                        : try await client.page(server, library: library, genre: nil, start: 0, length: ShelfLength.server)
                })
            case let .serverSearch(server):
                return .success(try await serverClient.search(server, words: search ?? ""))
            case let .tmdb(list, series):
                guard let tmdb else { return .failure(Problem("there is no TMDB key")) }
                return .success(try await tmdb.list(list, series: series, page: 1))
            case let .tmdbSearch(series):
                guard let tmdb else { return .failure(Problem("there is no TMDB key")) }
                return .success(try await tmdb.search(series: series, words: search ?? ""))
            }
        } catch {
            return .failure(Problem(error.localizedDescription))
        }
    }

    /// Fills a shelf, from a fresh answer or (`fromKept`) from what it held
    /// last time. A fresh answer replaces the kept one, unless it failed or
    /// holds the same titles.
    private func loaded(_ board: ShelfBoard, generation: UInt64, shelf index: Int, _ result: Result<[MetaPreview], Problem>, fromKept: Bool) {
        guard board.generation.isCurrent(generation), board.shelves.indices.contains(index) else { return }
        var shelf = board.shelves[index]
        let showingKept = !fromKept && !shelf.loading && !shelf.items.isEmpty
        shelf.loading = false
        switch result {
        case .failure where showingKept:
            break // the kept titles stay when the fresh ones cannot be had
        case let .failure(problem):
            shelf.error = "Could not load this: \(problem.words)."
        case let .success(metas):
            // A server that gathers catalogues lists the same film more than once.
            let length = board.board == .servers ? ShelfLength.server : ShelfLength.shelf
            let list = Array(Titles.withoutRepeats(metas).prefix(length))
            if !fromKept && !shelf.keptName.isEmpty { kept.write(shelf.keptName, list) }
            shelf.error = ""
            let same = showingKept && shelf.items.count == list.count
                && zip(shelf.items, list).allSatisfy { $0.id == $1.id && $0.name == $1.name }
            if !same {
                if board.board != .search { offerHero(list) }
                shelf.items = list
            }
        }
        board.shelves[index] = shelf
    }

    // MARK: Search

    func clearSearch() {
        fill(searchBoard, [])
        searchNote = ""
    }

    /// Asks every searchable catalogue, every media server and TMDB for
    /// `query`; each answers as a shelf.
    func runSearch(_ query: String) {
        let query = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return clearSearch() }
        var plans = installed.flatMap { source in
            source.catalogs.filter(\.searchable).map {
                ShelfPlan(title: $0.resultsTitle, source: source.name, from: .catalogue(url: source.url, catalog: $0))
            }
        }
        plans += servers.enabled.map {
            ShelfPlan(title: "Results on \($0.name)", source: $0.kind.label, from: .serverSearch($0))
        }
        if tmdb != nil {
            plans += [("Films", false), ("Series", true)].map {
                ShelfPlan(title: $0.0, source: "TMDB", from: .tmdbSearch(series: $0.1))
            }
        }
        searchNote = Self.searchNote(sources: installed.count, plans: plans.count)
        fill(searchBoard, plans, search: query)
    }

    nonisolated static func searchNote(sources: Int, plans: Int) -> String {
        guard plans == 0 else { return "" }
        #if ADDONS
        return sources == 0
            ? "Add a media server or a TMDB key in Settings, or install an add-on: search looks through those."
            : "None of your add-ons can be searched."
        #else
        return sources == 0
            ? "Add a media server or a TMDB key in Settings: search looks through those."
            : "Nothing you have added can be searched."
        #endif
    }

    // MARK: Media servers

    func loadServers() {
        servers = ServerAccounts.load(store)
        serverStates = serverStates.filter { id, _ in servers.enabled.contains { $0.id == id } }
        reloadServerShelves()
    }

    /// Asks every server for its libraries; each library then gets two
    /// shelves, the newest titles and the titles by name. The libraries
    /// found last time show at once.
    func reloadServerShelves() {
        let list = servers.enabled
        guard !list.isEmpty else {
            serverLibraries = []
            explore.rebuild()
            fill(serverBoard, [])
            return
        }
        let keptLists: [(server: Server, libraries: [MediaLibrary])]? = list.reduce(into: Optional([])) { all, server in
            guard all != nil, let libraries = kept.read(Kept.libraries(server), as: [MediaLibrary].self) else {
                all = nil
                return
            }
            all?.append((server, libraries))
        }
        var shown: [String]?
        if let keptLists {
            let plans = ServerAccounts.libraryPlans(keptLists)
            serverLibraries = keptLists.flatMap { entry in entry.libraries.map { (entry.server, $0) } }
            explore.rebuild()
            shown = plans.map { "\($0.title)|\($0.source)" }
            fill(serverBoard, plans.map(Self.plan))
        }
        Task {
            var listed: [(server: Server, libraries: [MediaLibrary])] = []
            for server in list {
                let client = serverClient
                let libraries = await withTimeout(ServerAccounts.checkWait) { try? await client.libraries(server) } ?? nil
                serverStates[server.id] = libraries == nil ? .offline : .online
                guard let libraries else {
                    Log.note("could not load from \(server.name)")
                    continue
                }
                kept.write(Kept.libraries(server), libraries)
                listed.append((server, libraries))
            }
            let plans = ServerAccounts.libraryPlans(listed)
            // The same shelves as the kept ones on screen: those stay.
            if shown == plans.map({ "\($0.title)|\($0.source)" }) { return }
            serverLibraries = listed.flatMap { entry in entry.libraries.map { (entry.server, $0) } }
            explore.rebuild()
            fill(serverBoard, plans.map(Self.plan))
        }
    }

    private static func plan(_ plan: ServerAccounts.ShelfPlan) -> ShelfPlan {
        ShelfPlan(title: plan.title, source: plan.source, from: .server(plan.server, library: plan.library, latest: plan.latest))
    }

    // MARK: Details

    /// How to fetch a title's details from where it came from: its media
    /// server, TMDB, or the first source that knows it; nil when nothing
    /// can. The answer is kept for the next start.
    func detailsOf(kind: String, id: String) -> (@MainActor () async throws -> MetaDetail)? {
        let fetch: (() async throws -> MetaDetail)?
        if let (server, item) = ServerAccounts.serverItem(id, in: servers.enabled) {
            let client = serverClient
            fetch = { try await client.meta(server, item: item) }
        } else if let (series, number) = TMDB.splitID(id) {
            guard let tmdb else { return nil }
            fetch = { try await tmdb.title(series: series, number: number) }
        } else {
            let sources = installed.filter { $0.provides("meta", kind: kind, id: id) }.map(\.url)
            guard !sources.isEmpty else { return nil }
            let catalogues = catalogues
            fetch = {
                var last: Error = Problem("no source has it")
                for url in sources {
                    do { return try await catalogues.meta(url, kind: kind, id: id) } catch { last = error }
                }
                throw last
            }
        }
        guard let fetch else { return nil }
        let kept = kept
        return {
            let meta = try await fetch()
            kept.write(Kept.details(kind: kind, id: id), meta)
            return meta
        }
    }

    func keptDetails(kind: String, id: String) -> MetaDetail? {
        kept.read(Kept.details(kind: kind, id: id), as: MetaDetail.self)
    }

    /// A film watched to the end (a series has no single tick).
    func filmWatched(kind: String, id: String) -> Bool {
        _ = marks
        return kind == "movie" && !((try? store.finishedVideos(kind: kind, titleID: id)) ?? []).isEmpty
    }

    // MARK: The banner

    /// Fills the banner from the first shelf that loads with 3 titles or more.
    private func offerHero(_ metas: [MetaPreview]) {
        guard !heroStarted, metas.count >= 3 else { return }
        heroStarted = true
        let chosen = Array(metas.prefix(HomeTop.slides))
        hero = chosen.map { HomeTop.Slide(id: $0.id, kind: $0.kind, title: $0.name, facts: $0.releaseInfo ?? "") }
        for (slot, meta) in chosen.enumerated() {
            // Details kept from last time show at once; fresh ones follow.
            let keptMeta = keptDetails(kind: meta.kind, id: meta.id)
            if let keptMeta {
                heroSettled += 1
                heroDetails(slot, keptMeta)
            }
            guard let fetch = detailsOf(kind: meta.kind, id: meta.id) else {
                if keptMeta == nil { heroSettled += 1 }
                continue
            }
            Task {
                let found = try? await fetch()
                if keptMeta == nil { heroSettled += 1 }
                if let found, found != keptMeta { heroDetails(slot, found) }
            }
        }
    }

    private func heroDetails(_ slot: Int, _ meta: MetaDetail) {
        guard hero.indices.contains(slot) else { return }
        hero[slot].facts = HomeTop.bannerFacts(meta, scores: nil)
        hero[slot].description = meta.description ?? ""
        hero[slot].scores = HomeTop.scorePills(meta, tmdb: nil, scores: nil)
        if let logo = meta.logo { hero[slot].logo = logo }
        if let image = meta.background ?? meta.poster { hero[slot].image = image }
        heroScores(slot, meta)
    }

    /// Adds the TMDB, Rotten Tomatoes and Metacritic scores (and the age
    /// rating), where keys for them are set. OMDb's answers are kept a week.
    private func heroScores(_ slot: Int, _ meta: MetaDetail) {
        guard let imdb = meta.imdbID ?? (TMDB.isIMDbID(meta.id) ? meta.id : nil) else { return }
        let (tmdb, omdbKey, kept, http, series, now) = (tmdb, keys.omdbAPIKey, kept, http, meta.kind == "series", now())
        Task {
            async let rating = try? await tmdb?.details(series: series, imdbID: imdb)?.rating
            async let scores = kept.scores(imdb: imdb, key: omdbKey, now: now, http: http)
            let (tmdbRating, omdb) = await (rating ?? nil, scores)
            guard hero.indices.contains(slot), hero[slot].id == meta.id || hero[slot].title == meta.name else { return }
            hero[slot].facts = HomeTop.bannerFacts(meta, scores: omdb)
            hero[slot].scores = HomeTop.scorePills(meta, tmdb: tmdbRating, scores: omdb)
        }
    }

    // MARK: Continue Watching

    /// Rebuilds Continue Watching from what was watched.
    func reloadContinue() {
        reloadHistory()
        let watched = (try? store.continueWatching(limit: HomeTop.continueLength)) ?? []
        // Details kept from last time, until the fresh ones come.
        for progress in watched {
            let key = Releases.Title(kind: progress.kind, id: progress.titleID)
            if continueKnown[key] == nil, let meta = keptDetails(kind: key.kind, id: key.id) {
                continueKnown[key] = (meta.background, meta.videos)
            }
        }
        continueCards = HomeTop.cards(watched, known: continueKnown, finished: finished, today: Day.today(zone).description)
        // Details (wide picture, episode list) once per title per run.
        for progress in watched {
            let key = Releases.Title(kind: progress.kind, id: progress.titleID)
            guard continueAsked.insert(key).inserted, let fetch = detailsOf(kind: key.kind, id: key.id) else { continue }
            Task {
                guard let meta = try? await fetch() else { return }
                let fresh = (meta.background, meta.videos)
                if let before = continueKnown[key], before.background == fresh.0, before.videos == fresh.1 { return }
                continueKnown[key] = fresh
                let watched = (try? store.continueWatching(limit: HomeTop.continueLength)) ?? []
                continueCards = HomeTop.cards(watched, known: continueKnown, finished: finished, today: Day.today(zone).description)
            }
        }
    }

    private func finished(_ progress: WatchProgress) -> Set<String> {
        Set((try? store.finishedVideos(kind: progress.kind, titleID: progress.titleID)) ?? [])
    }

    /// Takes a title off Continue Watching: forgets how far it (and, for a
    /// series, each of its episodes) was watched.
    func removeContinue(kind: String, id: String) {
        do {
            try store.removeProgress(kind: kind, titleID: id)
        } catch {
            Log.note("could not clear what was watched: \(error.localizedDescription)")
        }
        reloadContinue()
    }

    // MARK: Watchlist

    /// Rebuilds the Watchlist from the database. Titles saved without a
    /// poster (brought in from Trakt or Simkl, say) are looked up, so their
    /// pictures show too; the poster found is saved with the title.
    func reloadWatchlist() {
        watchlist = (try? store.watchlist()) ?? []
        for (index, saved) in watchlist.enumerated() where saved.poster == nil {
            if let poster = keptDetails(kind: saved.kind, id: saved.titleID)?.poster {
                setPoster(index, saved, poster)
                continue
            }
            let fetch = detailsOf(kind: saved.kind, id: saved.titleID)
            // An IMDb title nobody has details (or a poster) for yet, such as
            // a brand-new series: the picture service Cinemeta itself uses.
            let fallback = TMDB.isIMDbID(saved.titleID) ? "https://images.metahub.space/poster/medium/\(saved.titleID)/img" : nil
            guard fetch != nil || fallback != nil else { continue }
            Task {
                let found = (try? await fetch?())?.poster
                guard let url = found ?? fallback else { return }
                setPoster(index, saved, url)
            }
        }
        refreshCalendar()
    }

    private func setPoster(_ index: Int, _ saved: Saved, _ poster: String) {
        var updated = saved
        updated.poster = poster
        try? store.addToWatchlist(updated)
        if watchlist.indices.contains(index), watchlist[index].titleID == saved.titleID, watchlist[index].kind == saved.kind {
            watchlist[index].poster = poster
        }
    }

    // MARK: When things come out

    /// Brings the release dates in line with the Watchlist: asks for the
    /// details of titles not asked for yet (2 at a time), forgets titles
    /// taken off. Home's Upcoming row is made from them.
    func refreshCalendar() {
        let listed = Set(watchlist.map { Releases.Title(kind: $0.kind, id: $0.titleID) })
        releases = releases.filter { listed.contains($0.key) }
        calendarAsked = calendarAsked.filter { listed.contains($0) }
        for title in listed where calendarAsked.insert(title).inserted {
            // The dates found last time show at once; fresh ones follow.
            let keptMeta = keptDetails(kind: title.kind, id: title.id)
            if let keptMeta {
                if let picture = keptMeta.background ?? keptMeta.poster { calendarPictures[title] = picture }
                releases[title] = Releases.of(keptMeta, zone: zone)
            }
            guard let fetch = detailsOf(kind: title.kind, id: title.id) else { continue }
            let counted = keptMeta == nil
            if counted { calendarWaiting += 1 }
            fetchesLeft += 1
            let gate = calendarGate
            Task {
                await gate.enter()
                let found = try? await fetch()
                await gate.leave()
                if counted { calendarWaiting = max(calendarWaiting - 1, 0) }
                fetchesLeft = max(fetchesLeft - 1, 0)
                // Taken off the Watchlist meanwhile: not wanted.
                guard calendarAsked.contains(title) else { return }
                if let found {
                    if let picture = found.background ?? found.poster { calendarPictures[title] = picture }
                    releases[title] = Releases.of(found, zone: zone)
                } else {
                    // Asked again next time (the kept dates stay meanwhile).
                    Log.note("calendar: no dates for a title")
                    calendarAsked.remove(title)
                }
                updateUpcoming()
            }
        }
        updateUpcoming()
    }

    /// Asks every title's dates again (Home pulled down).
    func recheckCalendar() {
        calendarAsked.removeAll()
        refreshCalendar()
    }

    private func updateUpcoming() {
        let names = Dictionary(watchlist.map { (Releases.Title(kind: $0.kind, id: $0.titleID), $0.title) }, uniquingKeysWith: { a, _ in a })
        upcoming = Releases.upcoming(releases, names: names, pictures: calendarPictures, today: Day.today(zone))
    }

    /// Every release known, by title (for the Calendar page, S16).
    var allReleases: [Releases.Title: [Release]] { releases }

    // MARK: Pull to refresh

    /// Loads Home afresh: every shelf, the servers' libraries, the dates and
    /// Continue Watching. What is on screen stays until each fresh answer
    /// replaces it. Returns once nothing is still out (at most 20 s).
    func refreshHome() async {
        Log.note("home: loading afresh (pulled down)")
        refreshing = true
        reloadHome()
        reloadCatalog()
        reloadServerShelves()
        recheckCalendar()
        continueAsked.removeAll()
        reloadContinue()
        let started = Date()
        while fetchesLeft > 0, Date().timeIntervalSince(started) < 20 {
            try? await Task.sleep(for: .milliseconds(250))
        }
        refreshing = false
    }

    // MARK: Loading screen

    /// How far Haven has got with what it loads at start, 0 to 1: Home's
    /// shelves, the banner's details and its first picture, and the dates.
    /// A failed request counts as done, so the screen never waits for an
    /// answer that will not come.
    func startupProgress(homeShowing: Bool) -> Double {
        var parts: [Double] = []
        if homeShowing {
            let boards = [homeBoard, serverBoard, catalogBoard]
            let total = boards.reduce(0) { $0 + $1.shelves.count }
            let done = boards.reduce(0) { $0 + $1.settled }
            let hasSources = !installed.isEmpty || !servers.enabled.isEmpty
            parts.append(total == 0 ? (hasSources ? 0 : 1) : Double(done) / Double(total))
            if heroStarted && !hero.isEmpty {
                parts.append(Double(min(heroSettled, hero.count)) / Double(hero.count) * 0.7 + (heroFirstPicture ? 0.3 : 0))
            } else if total > 0 && done < total {
                parts.append(0)
            }
        }
        if !calendarAsked.isEmpty {
            parts.append(Double(calendarAsked.count - min(calendarWaiting, calendarAsked.count)) / Double(calendarAsked.count))
        }
        return parts.isEmpty ? 1 : parts.reduce(0, +) / Double(parts.count)
    }

    /// Home has something to show (else it explains how to fill it).
    var homeHasSources: Bool {
        !homeBoard.shelves.isEmpty || !servers.enabled.isEmpty || !catalogBoard.shelves.isEmpty
    }
}
