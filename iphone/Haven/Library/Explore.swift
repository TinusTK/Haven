import Foundation
import Observation

/// Somewhere Explore can browse: the built-in TMDB catalogue, a library of a
/// media server, or a catalogue of Cinemeta or an add-on.
enum ExploreSource: Equatable, Sendable {
    case tmdb
    case library(Server, MediaLibrary)
    case catalogue(url: String, source: String, catalog: Catalog)

    /// Where it comes from: the heading it is listed under.
    var origin: String {
        switch self {
        case .tmdb: "TMDB"
        case let .library(server, _): server.name
        case let .catalogue(_, source, _): source
        }
    }

    /// Its name under that heading.
    var name: String {
        switch self {
        case .tmdb: "Films and series"
        case let .library(_, library): library.name
        case let .catalogue(_, _, catalog): catalog.title
        }
    }

    /// Tells sources apart, whatever their place in the list.
    var label: String { "\(name)  ·  \(origin)" }
}

/// A line in the list of sources: a heading, or a source by its place.
struct ExploreRow: Equatable, Identifiable, Sendable {
    /// The source's place in the list; -1 for a heading.
    var index: Int
    var label: String
    var header: Bool { index < 0 }
    var id: String { header ? "h|\(label)" : "\(index)" }
}

/// The Explore page: one grid of posters to browse everything there is, a
/// source at a time, narrowed to films or series (TMDB) and a genre. The
/// Rust build's `library/explore.rs`.
@MainActor @Observable
final class Explore {
    /// Titles asked for at a time from a media server.
    nonisolated static let serverPage = 60
    /// TMDB hands out 20 titles a page; two pages fill a screen.
    nonisolated static let tmdbPagesAtOnce = 2

    @ObservationIgnored private weak var library: Library?

    // What the page shows.
    private(set) var sources: [ExploreSource] = []
    /// Words the listed sources must hold.
    var words = "" { didSet { if words != oldValue { listRows() } } }
    private(set) var rows: [ExploreRow] = []
    /// The chosen source's place in `sources`; -1 when there is none.
    private(set) var source = -1
    /// "Films", "Series" for TMDB; empty for the others.
    private(set) var kinds: [String] = []
    /// 0 films, 1 series (TMDB only).
    private(set) var kind = 0
    /// "All genres" then the source's genres; empty when there are none.
    private(set) var genres: [String] = []
    /// The chosen genre's place (without "All genres"); -1 for all.
    private(set) var genre = -1
    private(set) var items: [MetaPreview] = []
    private(set) var loading = false
    /// There may be another page.
    private(set) var more = false
    private(set) var note = ""
    private(set) var refreshing = false

    // The search and the source list on top of the grid (kept while other
    // pages show, as in the Rust build).
    var query = ""
    var searching = false
    var picking = false

    // Behind the page.
    /// The chosen source's label, to find it again when the list changes.
    private var chosen = ""
    private var tmdbGenres: [Genre] = []
    private var serverGenres: [String] = []
    private let generation = Generation()
    /// Where the next page starts: a page number for TMDB, else how many
    /// titles to skip.
    private var next = 0
    /// The first page of this source has not come yet.
    private var first = false
    /// The grid shows the first page kept from last time (or the titles
    /// that showed before a refresh).
    private var showingKept = false
    private var keptName = ""
    /// The page has been opened: nothing is fetched before that.
    private var wanted = false

    init(library: Library) {
        self.library = library
    }

    // MARK: Sources

    /// Lists the sources again (an add-on, a server or the TMDB key came or
    /// went). The chosen source stays chosen if it is still there.
    func rebuild() {
        guard let library else { return }
        var list: [ExploreSource] = []
        if library.tmdb != nil { list.append(.tmdb) }
        list += library.serverLibraries.map { ExploreSource.library($0.server, $0.library) }
        for found in library.installed {
            list += found.catalogs.filter(\.browsable).map { .catalogue(url: found.url, source: found.name, catalog: $0) }
        }
        let still = list.firstIndex { $0.label == chosen }
        sources = list
        listRows()
        if let still {
            // Still there: only its place in the list may have moved.
            source = still
        } else {
            pick(0, kind: 0, genre: -1)
        }
    }

    /// The list of sources: a heading for each place they come from, then
    /// its sources that hold the words looked for.
    private func listRows() {
        let wanted = words.lowercased().split(whereSeparator: \.isWhitespace)
        var list: [ExploreRow] = []
        var heading: String?
        for (index, found) in sources.enumerated() {
            let label = found.label.lowercased()
            guard wanted.allSatisfy({ label.contains($0) }) else { continue }
            if found.origin != heading {
                heading = found.origin
                list.append(ExploreRow(index: -1, label: found.origin))
            }
            list.append(ExploreRow(index: index, label: found.name))
        }
        rows = list
    }

    // MARK: Choosing

    /// Shows a source from its start, narrowed to a kind and a genre.
    func pick(_ index: Int, kind: Int, genre: Int) {
        generation.next()
        items = []
        loading = false
        guard sources.indices.contains(max(index, 0)) else {
            chosen = ""
            source = -1
            kinds = []
            genres = []
            note = ""
            more = false
            return
        }
        let picked = sources[max(index, 0)]
        let sameKind = chosen == picked.label && self.kind == max(kind, 0)
        chosen = picked.label
        source = max(index, 0)
        self.kind = max(kind, 0)
        self.genre = genre
        note = ""
        switch picked {
        case .tmdb:
            next = 1
            kinds = ["Films", "Series"]
            if !sameKind {
                // Another kind has other genres: fetch them.
                tmdbGenres = []
                genres = []
                fetchTMDBGenres(series: self.kind == 1)
            }
        case let .library(server, mediaLibrary):
            next = 0
            kinds = []
            if !sameKind {
                serverGenres = []
                genres = []
                fetchServerGenres(server, mediaLibrary, label: picked.label)
            }
        case let .catalogue(_, _, catalog):
            next = 0
            kinds = []
            genres = Self.choices(catalog.genres)
            // A catalogue that only answers with a genre starts on its first.
            if genre < 0 && catalog.needsInput { self.genre = 0 }
        }
        // The first page as it was last time shows at once; the fresh one
        // replaces it when it comes.
        keptName = Kept.explore(label: picked.label, kind: String(self.kind), genre: String(self.genre))
        first = true
        showingKept = false
        if let titles = library?.kept.read(keptName, as: [MetaPreview].self), !titles.isEmpty {
            add(titles)
            showingKept = true
        }
        if wanted { load() }
    }

    func chooseSource(_ index: Int) {
        pick(index, kind: 0, genre: -1)
        picking = false
    }

    func chooseKind(_ kind: Int) {
        pick(source, kind: kind, genre: -1)
    }

    /// `choice` is the place in `genres`: 0 is "All genres".
    func chooseGenre(_ choice: Int) {
        pick(source, kind: kind, genre: choice - 1)
    }

    /// The page came into view: the grid starts loading the first time.
    func appeared() {
        guard !wanted else { return }
        wanted = true
        if first && !loading { load() }
    }

    /// The genres as the chooser lists them: "All genres" first, which
    /// stands for no genre. Empty when there are none to choose from.
    nonisolated static func choices(_ names: [String]) -> [String] {
        names.isEmpty ? [] : ["All genres"] + names
    }

    private func fetchTMDBGenres(series: Bool) {
        guard let tmdb = library?.tmdb else { return }
        Task {
            guard let found = try? await tmdb.genres(series: series) else { return }
            // Still the same source and kind.
            guard kind == (series ? 1 : 0), chosen == ExploreSource.tmdb.label else { return }
            tmdbGenres = found
            genres = Self.choices(found.map(\.name))
        }
    }

    private func fetchServerGenres(_ server: Server, _ mediaLibrary: MediaLibrary, label: String) {
        guard let client = library?.serverClient else { return }
        Task {
            let found = await client.genres(server, library: mediaLibrary.id)
            guard chosen == label, !found.isEmpty else { return }
            serverGenres = found
            genres = Self.choices(found)
        }
    }

    // MARK: Pages

    /// Loads the source showing afresh (Explore pulled down). The titles on
    /// screen stay until the fresh first page replaces them; returns when it
    /// has come (or after 20 seconds).
    func refresh() async {
        guard sources.indices.contains(source), sources[source].label == chosen else { return }
        refreshing = true
        let onScreen = items
        let wasKept = showingKept
        pick(source, kind: kind, genre: genre)
        if !onScreen.isEmpty && !wasKept && !showingKept {
            items = onScreen
            showingKept = true
        }
        let started = Date()
        while first, Date().timeIntervalSince(started) < 20 {
            try? await Task.sleep(for: .milliseconds(150))
        }
        refreshing = false
    }

    /// Fetches the next page of the chosen source and adds it to the grid.
    func load() {
        guard !loading, let library, sources.indices.contains(source), sources[source].label == chosen else { return }
        let picked = sources[source]
        let token = generation.current
        let (start, genre, series) = (next, genre, kind == 1)
        loading = true
        let tmdbGenre = tmdbGenres.indices.contains(genre) ? tmdbGenres[genre].id : nil
        let serverGenre = serverGenres.indices.contains(genre) ? serverGenres[genre] : nil
        let (tmdb, client, catalogues) = (library.tmdb, library.serverClient, library.catalogues)
        Task {
            let page = await Self.fetch(picked, start: start, genre: genre, series: series, tmdbGenre: tmdbGenre,
                                        serverGenre: serverGenre, tmdb: tmdb, client: client, catalogues: catalogues)
            loaded(token, page)
        }
    }

    /// A page of titles, where the page after it starts, and whether there
    /// may be one.
    struct Page: Equatable, Sendable {
        var titles: [MetaPreview]
        var next: Int
        var more: Bool
    }

    nonisolated static func fetch(_ source: ExploreSource, start: Int, genre: Int, series: Bool, tmdbGenre: Int64?,
                                  serverGenre: String?, tmdb: TMDB?, client: ServerClient,
                                  catalogues: Catalogues) async -> Result<Page, Problem> {
        do {
            switch source {
            case .tmdb:
                guard let tmdb else { return .failure(Problem("there is no TMDB key")) }
                var titles: [MetaPreview] = []
                for number in start..<(start + tmdbPagesAtOnce) {
                    do {
                        titles += try await tmdb.discover(series: series, genre: tmdbGenre, page: number)
                    } catch {
                        if titles.isEmpty { throw error }
                        break
                    }
                }
                return .success(Page(titles: titles, next: start + tmdbPagesAtOnce, more: titles.count >= 20 * tmdbPagesAtOnce))
            case let .library(server, mediaLibrary):
                let titles = try await client.page(server, library: mediaLibrary.id, genre: serverGenre, start: start, length: serverPage)
                return .success(Page(titles: titles, next: start + serverPage, more: titles.count >= serverPage))
            case let .catalogue(url, _, catalog):
                let named = catalog.genres.indices.contains(genre) ? catalog.genres[genre] : nil
                let titles = try await catalogues.catalogPage(url, catalog, genre: named, skip: start)
                return .success(Page(titles: titles, next: start + titles.count, more: catalog.pages && !titles.isEmpty))
            }
        } catch {
            return .failure(Problem(HTTP.words(error)))
        }
    }

    private func loaded(_ token: UInt64, _ result: Result<Page, Problem>) {
        guard generation.isCurrent(token) else { return }
        loading = false
        let wasFirst = first
        first = false
        let page: Page
        switch result {
        case let .success(found):
            page = found
        case .failure where wasFirst && showingKept:
            // The kept page stays when the fresh one cannot be had.
            showingKept = false
            more = false
            return
        case let .failure(problem):
            more = false
            note = "Could not load this: \(problem.words)."
            return
        }
        next = page.next
        if wasFirst {
            library?.kept.write(keptName, page.titles)
            // The fresh first page takes the kept one's place.
            if showingKept {
                showingKept = false
                items = []
            }
        }
        let added = add(page.titles)
        // A page of nothing new means the end, whatever the source says.
        more = page.more && added > 0
        note = items.isEmpty ? "Nothing here." : ""
    }

    /// Adds titles to the grid, each once; returns how many were added.
    @discardableResult
    private func add(_ titles: [MetaPreview]) -> Int {
        let (list, added) = Self.adding(titles, to: items)
        items = list
        return added
    }

    /// The grid with `titles` added: pages can overlap, and the same film
    /// can be listed again on a later page under another id.
    nonisolated static func adding(_ titles: [MetaPreview], to items: [MetaPreview]) -> ([MetaPreview], Int) {
        var list = items
        var ids = Set(items.map { "\($0.kind)|\($0.id)" })
        var keys = Set(items.map { Titles.titleKey(kind: $0.kind, name: $0.name, year: $0.releaseInfo) })
        var added = 0
        for title in Titles.withoutRepeats(titles) {
            let key = Titles.titleKey(kind: title.kind, name: title.name, year: title.releaseInfo)
            guard ids.insert("\(title.kind)|\(title.id)").inserted, keys.insert(key).inserted else { continue }
            list.append(title)
            added += 1
        }
        return (list, added)
    }
}
