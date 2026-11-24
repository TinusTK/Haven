import Foundation

/// A change made in Haven that is still to be sent to the linked
/// accounts: "watched", "unwatched", "watchlist_add" or "watchlist_remove".
struct Change: Equatable, Sendable {
    var id: Int64 = 0
    var what: String
    var kind: String
    var titleID: String
    var videoID: String
    var createdAt: Int64
}

/// A film or episode saved to this device, or on its way.
struct Download: Equatable, Sendable {
    /// Its number here; 0 before it is saved.
    var id: Int64 = 0
    /// "movie" or "series".
    var kind: String
    var titleID: String
    /// The film's or episode's own id.
    var videoID: String
    /// The film's or series' name.
    var title: String
    /// For an episode, which one ("S1 E2 · Name"); empty for a film.
    var detail: String
    var season: Int?
    var episode: Int?
    var poster: String?
    /// Where it is fetched from.
    var url: String
    /// The file it is saved to.
    var path: String
    /// Its whole size in bytes; 0 while unknown.
    var size: Int64
    /// Bytes saved so far.
    var received: Int64
    /// "waiting", "loading", "done", "failed" or "paused".
    var state: String
    /// In milliseconds since 1970.
    var addedAt: Int64
}

#if ADDONS
/// An installed add-on: where it lives and the manifest it served when it
/// was installed, as raw JSON.
struct StoredAddon: Equatable, Sendable {
    var url: String
    var manifest: String
}
#endif

/// How far one film or episode has been watched.
struct WatchProgress: Equatable, Sendable {
    /// Content type: "movie", "series"…
    var kind: String
    /// The title's id (for an episode, its series).
    var titleID: String
    /// The film's or episode's own id; what streams are asked for.
    var videoID: String
    /// The title's name, for showing without asking again.
    var title: String
    /// For an episode, which one ("S1 E2 · Name"); empty for a film.
    var detail: String
    /// Address of the title's poster, if known.
    var poster: String?
    /// Seconds from the start.
    var position: Double
    /// Running time in seconds.
    var duration: Double
    /// Watched to the end (or near enough).
    var finished: Bool
    /// When this was saved, in milliseconds since 1970.
    var updatedAt: Int64
}

/// A media server the user signed in to.
struct StoredServer: Equatable, Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    /// Its number here; 0 before it is saved.
    var id: Int64 = 0
    /// "jellyfin", "emby" or "plex".
    var kind: String
    var name: String
    var address: String
    var userID: String
    /// What the server gave in exchange for signing in. The password is
    /// never kept.
    var token: String

    /// Never shows the token.
    var description: String {
        "StoredServer(id: \(id), kind: \(kind), name: \(name), address: \(address), ..)"
    }
    var debugDescription: String { description }
}

/// A source of television channels and radio stations: a playlist's
/// address and the playlist as it was last fetched.
struct TvSource: Equatable, Sendable {
    var url: String
    var name: String
    /// The playlist's text, kept so channels show before the network answers.
    var playlist: String
}

/// A title saved to watch later.
struct Saved: Equatable, Sendable {
    /// Content type: "movie", "series"…
    var kind: String
    var titleID: String
    var title: String
    /// Year or years, as given; may be empty.
    var year: String
    /// Address of the poster, if known.
    var poster: String?
    /// When it was saved, in milliseconds since 1970.
    var addedAt: Int64
}
