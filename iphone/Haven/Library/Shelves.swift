import Foundation
import Observation

/// The places that show shelves.
enum Board: String, Sendable {
    /// The catalogues' shelves on Home (Cinemeta, add-ons).
    case home = "Home"
    case search = "Search"
    /// The media servers' shelves on Home.
    case servers = "Servers"
    /// The TMDB catalogue's shelves on Home.
    case catalog = "Catalog"
}

/// A row of posters with a heading.
struct Shelf: Identifiable, Equatable, Sendable {
    /// Its place on the board.
    var id: Int
    var title: String
    /// Where its titles come from ("Cinemeta", a server's name, "TMDB").
    var source: String
    var loading = true
    var error = ""
    var items: [MetaPreview] = []
    /// The name it is kept under for the next start; empty: not kept.
    var keptName = ""
}

/// Where a shelf's titles come from.
enum ShelfFrom: Sendable {
    /// A catalogue of a source (Cinemeta or an add-on), by the source's address.
    case catalogue(url: String, catalog: Catalog)
    /// A library on a media server: its newest titles, or all by name.
    case server(Server, library: String, latest: Bool)
    /// A media server's answer to the search words.
    case serverSearch(Server)
    /// One of TMDB's ready-made lists of films or series.
    case tmdb(TMDBList, series: Bool)
    /// TMDB's answer to the search words.
    case tmdbSearch(series: Bool)
}

/// A shelf to build.
struct ShelfPlan: Sendable {
    var title: String
    var source: String
    var from: ShelfFrom
}

/// One board's shelves. Rebuilding it starts a new generation, so late
/// answers for the old shelves are dropped.
@MainActor @Observable
final class ShelfBoard {
    let board: Board
    var shelves: [Shelf] = []
    let generation = Generation()

    init(_ board: Board) {
        self.board = board
    }

    /// How many shelves have answered (for the loading screen).
    var settled: Int { shelves.filter { !$0.loading }.count }
}

/// What the shelves show: the start of a list (Explore shows it all).
enum ShelfLength {
    static let shelf = 30
    /// A media server's library shelf.
    static let server = 60
    /// Media servers' shelves fetched at once (a server that gathers
    /// catalogues refuses bursts).
    static let serversAtOnce = 2
}

/// The TMDB shelves on Home, in order (only with a TMDB key).
enum TMDBShelves {
    static let plans: [(title: String, list: TMDBList, series: Bool)] = [
        ("Trending films", .trending, false),
        ("Trending series", .trending, true),
        ("In cinemas now", .now, false),
        ("On television now", .now, true),
        ("Popular films", .popular, false),
        ("Popular series", .popular, true),
        ("Coming soon", .upcoming, false),
        ("Top rated films", .topRated, false),
        ("Top rated series", .topRated, true),
    ]
}
