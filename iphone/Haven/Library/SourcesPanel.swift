import Foundation
import Observation

/// Where a film or episode can be watched: every version of it on every
/// media server that has it, every stream the add-ons offer for it, and the
/// Xtream Codes accounts' films and series. The Rust build's
/// `library/sources.rs`.
@MainActor @Observable
final class SourcesPanel {
    /// The film or episode listed.
    struct Listed: Equatable {
        var videoID: String
        /// For an episode, which one ("S1 E2 · Name"); empty for a film.
        var detail = ""
    }

    /// What is asked about: the open title and the film or episode.
    struct Asking {
        var kind: String
        var titleID: String
        var videoID: String
        var meta: MetaDetail?
        var name: String
    }

    @ObservationIgnored private weak var library: Library?

    private(set) var heading = TitleRules.heading
    private(set) var loading = false
    private(set) var note = ""
    private(set) var rows: [StreamRow] = []
    private(set) var groups: [(name: String, count: Int)] = []
    /// The provider chosen above the list; empty for all.
    var group = ""
    private(set) var listed: Listed?

    /// The next episode plays from the same place as the last (its source
    /// and the row's name) as soon as that row shows.
    @ObservationIgnored var autoplay: (source: String, name: String)?
    /// Plays a row (the title page's Play).
    @ObservationIgnored var playRow: (StreamRow) -> Void = { _ in }
    /// The next episode's own place did not turn up: the player gives way
    /// to this list.
    @ObservationIgnored var giveUp: () -> Void = {}

    private var serverRows = 0
    private var limits: [String: Int] = [:]
    private var pending = 0
    private var nextID = 0
    private let generation = Generation()
    /// Tells a long list still arriving from a media server to stop.
    private let current = Current()

    init(library: Library) {
        self.library = library
    }

    /// Empties the list (another title opened, or the page closed).
    func clear() {
        generation.next()
        current.set(generation.current)
        rows = []
        serverRows = 0
        limits = [:]
        pending = 0
        loading = false
        note = ""
        heading = TitleRules.heading
        listed = nil
        showGroups()
    }

    /// Says something over an empty list ("Choose an episode…").
    func say(_ words: String, heading: String = TitleRules.heading) {
        clear()
        note = words
        self.heading = heading
    }

    /// Lists every place the film or episode can be watched: asks each
    /// media server whether it has it, every add-on that offers streams for
    /// it, and the Xtream Codes accounts.
    func load(_ asking: Asking, heading: String) {
        clear()
        guard let library else { return }
        let token = generation.current
        listed = Listed(videoID: asking.videoID)
        self.heading = heading
        let wanted = TitleRules.wanted(kind: asking.kind, videoID: asking.videoID, openID: asking.titleID,
                                       meta: asking.meta, name: asking.name)
        // A series can only be matched down to an episode.
        let canMatch = !wanted.name.trimmingCharacters(in: .whitespaces).isEmpty && (!wanted.series || wanted.episode != nil)
        let servers = library.servers.enabled
        let origin = ServerAccounts.serverItem(asking.videoID, in: servers)
        // A title a server gathered under a TMDB or other number has no
        // sources under that name: it is looked up again, by its IMDb id.
        let renamed = Servers.splitID(asking.titleID).map { Servers.wantsIMDb($0.item) } ?? false
        // Where an IMDb id can be looked up by name when none is known.
        let lookups = library.installed.flatMap { source in
            source.catalogs.filter { $0.kind == asking.kind && $0.searchable }.map { (source.url, $0) }
        }
        var asked = 0
        let (client, catalogues, flag) = (library.serverClient, library.catalogues, current)

        for server in servers {
            // The server the title was opened from holds it for certain; the
            // others are asked whether they have the same title.
            let own = origin.flatMap { $0.server.id == server.id && !renamed ? $0.item : nil }
            // Every server is asked, even one marked offline earlier: it may
            // be back (user: refresh when the sources open).
            if own == nil && !canMatch { continue }
            asked += 1
            let source = "\(server.name) (\(server.kind.label))"
            let ask = wanted
            Task {
                var found: Result<String?, Problem>
                do {
                    var item = own
                    if item == nil {
                        var ask = ask
                        if ask.imdb == nil { ask.imdb = await Self.imdbByName(catalogues, lookups, ask) }
                        item = try await client.find(server, ask)
                    }
                    if let item {
                        let id = Servers.titleID(server: server.id, item: item)
                        // A long list shows as it arrives, a few at a time.
                        try await client.streamsLive(server, item: item) { @Sendable some in
                            Task { @MainActor in self.arrived(token, source: source, serverItem: id, some) }
                            return flag.is(token)
                        }
                        found = .success(id)
                    } else {
                        found = .success(nil)
                    }
                } catch {
                    found = .failure(Problem(HTTP.words(error)))
                }
                switch found {
                case .success:
                    // It answered: online (its shelves come back if it was not).
                    library.serverAnswered(server.id)
                    loaded(token, source: source, serverItem: nil, .success([]))
                case let .failure(problem): loaded(token, source: source, serverItem: nil, .failure(problem))
                }
            }
        }

        #if ADDONS
        if let addonID = TitleRules.addonID(videoID: asking.videoID, wanted: wanted) {
            let addons = AddonClient(catalogues: catalogues)
            for source in library.installed where source.provides("stream", kind: asking.kind, id: addonID) {
                asked += 1
                Task {
                    let result: Result<[MediaStream], Problem>
                    do {
                        result = .success(try await addons.streams(source.url, kind: asking.kind, id: addonID))
                    } catch {
                        result = .failure(Problem(HTTP.words(error)))
                    }
                    loaded(token, source: source.name, serverItem: nil, result)
                }
            }
        }
        #endif

        // The films and series of the user's Xtream Codes accounts.
        if XtreamOnDemand.worthAsking(wanted) {
            let onDemand = library.onDemand
            for account in LiveTV.accounts(library.store) {
                asked += 1
                let ask = wanted
                Task {
                    let result = await onDemand.streams(account, wanted: ask)
                    loaded(token, source: XtreamOnDemand.sourceLabel(account), serverItem: nil, result)
                }
            }
        }

        pending = asked
        if asked == 0 {
            giveUpAutoplay()
            note = TitleRules.nothingCanPlay
        } else {
            loading = true
        }
    }

    /// Looks a title's IMDb id up by its name and year in the catalogues
    /// that can be searched (the built-in one, for a start).
    nonisolated static func imdbByName(_ catalogues: Catalogues, _ lookups: [(String, Catalog)], _ wanted: Wanted) async -> String? {
        let name = wanted.name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return nil }
        for (url, catalog) in lookups {
            guard let found = try? await catalogues.catalog(url, catalog, search: name) else { continue }
            if let hit = TitleRules.imdbMatch(found, name: name, year: wanted.year) { return hit }
        }
        return nil
    }

    /// Some places to watch came in (a media server may send more).
    func arrived(_ token: UInt64, source: String, serverItem: String?, _ streams: [MediaStream]) {
        guard generation.isCurrent(token) else { return }
        for stream in streams {
            guard let row = TitleRules.streamRow(id: nextID, source: source, stream) else { continue }
            nextID += 1
            var placed = row
            placed.serverItem = serverItem
            let at = TitleRules.place(placed, in: rows, serverRows: serverRows, fromServer: serverItem != nil)
            rows.insert(placed, at: at)
            if serverItem != nil { serverRows += 1 }
        }
        rows = TitleRules.marked(rows, limits: limits)
        showGroups()
        autoplayFrom(source, finished: false)
    }

    /// Plays the next episode from `source` if that is where the last one
    /// played from: the row of the same name or, once the source has
    /// `finished` answering, its first playable row.
    private func autoplayFrom(_ source: String, finished: Bool) {
        guard let wanted = autoplay, wanted.source == source else { return }
        let candidates = rows.filter { $0.playable && $0.source == wanted.source }
        guard let pick = candidates.first(where: { $0.name == wanted.name }) ?? (finished ? candidates.first : nil) else { return }
        autoplay = nil
        playRow(pick)
    }

    private func giveUpAutoplay() {
        guard autoplay != nil else { return }
        autoplay = nil
        giveUp()
    }

    /// One media server, add-on or account has finished answering.
    func loaded(_ token: UInt64, source: String, serverItem: String?, _ result: Result<[MediaStream], Problem>) {
        guard generation.isCurrent(token) else { return }
        switch result {
        case let .success(streams): arrived(token, source: source, serverItem: serverItem, streams)
        case let .failure(problem): Log.note("\(source) gave no places to watch: \(problem.words)")
        }
        autoplayFrom(source, finished: true)
        pending = max(pending - 1, 0)
        if pending == 0 {
            giveUpAutoplay()
            loading = false
            note = rows.isEmpty ? TitleRules.noneHasIt : ""
        }
    }

    /// Shows ten more of a provider's places to watch.
    func showMore(_ group: String) {
        limits[group, default: TitleRules.streamsShown] += TitleRules.streamsShown
        rows = TitleRules.marked(rows, limits: limits)
    }

    /// The rows the list shows now: ten per provider, or the chosen one's.
    var shownRows: [StreamRow] {
        rows.filter { $0.shown && (group.isEmpty || $0.group == group) }
    }

    /// Media servers, add-ons or accounts still to answer.
    var stillAsking: Bool { pending > 0 }

    func setDetail(_ detail: String) {
        listed?.detail = detail
    }

    private func showGroups() {
        groups = TitleRules.groups(rows)
        // A chosen provider that is gone falls back to all.
        if !groups.contains(where: { $0.name == group }) { group = "" }
        if rows.isEmpty { limits = [:] }
    }
}

/// The list being read now, readable from any thread: a long list from a
/// media server stops arriving once another one is asked for.
final class Current: @unchecked Sendable {
    private let lock = NSLock()
    private var value: UInt64 = 0

    func set(_ token: UInt64) {
        lock.withLock { value = token }
    }

    func `is`(_ token: UInt64) -> Bool {
        lock.withLock { value == token }
    }
}
