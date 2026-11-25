import Foundation

/// Titles Haven knows by another id than the IMDb one (a media server's
/// own, say) linked to their IMDb id, which is how Trakt and Simkl know
/// them. With the link a film or episode watched on a service shows as
/// watched on the server's title, and one watched on the server's title
/// is sent to the services.
struct IMDbLink: Equatable {
    var imdb: String
    /// Episode ids against their season and episode numbers.
    var episodes: [String: (season: Int, episode: Int)]

    static func == (a: IMDbLink, b: IMDbLink) -> Bool {
        a.imdb == b.imdb && a.episodes.mapValues { "\($0.season):\($0.episode)" } == b.episodes.mapValues { "\($0.season):\($0.episode)" }
    }

    /// The IMDb form of an episode's id, as Haven keeps what services send.
    static func episodeID(_ imdb: String, _ numbers: (season: Int, episode: Int)) -> String {
        "\(imdb):\(numbers.season):\(numbers.episode)"
    }

    /// The link from a title's details (or a gathered server id).
    static func of(titleID: String, detail: MetaDetail?) -> IMDbLink? {
        guard !TMDB.isIMDbID(titleID) else { return nil }
        let fromDetail = detail?.imdbID.flatMap { TMDB.isIMDbID($0) ? $0 : nil }
        let fromID = Servers.splitID(titleID).flatMap { Servers.imdbIn($0.item) }
        guard let imdb = fromDetail ?? fromID else { return nil }
        var episodes: [String: (season: Int, episode: Int)] = [:]
        for video in detail?.videos ?? [] {
            if let season = video.season, let episode = video.episode { episodes[video.id] = (season, episode) }
        }
        return IMDbLink(imdb: imdb, episodes: episodes)
    }

    /// A title and video as the services know them; nil when an episode's
    /// numbers are not known.
    func serviceIDs(kind: String, videoID: String) -> (titleID: String, videoID: String)? {
        if videoID.isEmpty { return (imdb, "") }
        if kind != "series" { return (imdb, imdb) }
        guard let numbers = episodes[videoID] else { return nil }
        return (imdb, Self.episodeID(imdb, numbers))
    }

    /// Adds to `own` (a title's finished videos) those finished under the
    /// IMDb id (`elsewhere`), in the title's own ids.
    func merged(kind: String, titleID: String, own: Set<String>, elsewhere: Set<String>) -> Set<String> {
        var done = own
        if kind == "series" {
            for (id, numbers) in episodes where elsewhere.contains(Self.episodeID(imdb, numbers)) { done.insert(id) }
        } else if !elsewhere.isEmpty {
            done.insert(titleID)
        }
        return done
    }
}

extension Library {
    func imdbLink(kind: String, titleID: String) -> IMDbLink? {
        guard !TMDB.isIMDbID(titleID) else { return nil }
        return IMDbLink.of(titleID: titleID, detail: keptDetails(kind: kind, id: titleID))
    }

    /// A title and video as the services know them (IMDb ids); nil when
    /// there is none to go by.
    func serviceIDs(kind: String, titleID: String, videoID: String) -> (titleID: String, videoID: String)? {
        if TMDB.isIMDbID(titleID) { return (titleID, videoID) }
        return imdbLink(kind: kind, titleID: titleID)?.serviceIDs(kind: kind, videoID: videoID)
    }

    /// A title's films or episodes watched to the end: its own records and
    /// those kept under its IMDb id (from a service).
    func finishedOf(kind: String, titleID: String) -> Set<String> {
        let own = Set((try? store.finishedVideos(kind: kind, titleID: titleID)) ?? [])
        guard let link = imdbLink(kind: kind, titleID: titleID) else { return own }
        let elsewhere = Set((try? store.finishedVideos(kind: kind, titleID: link.imdb)) ?? [])
        return link.merged(kind: kind, titleID: titleID, own: own, elsewhere: elsewhere)
    }

    /// Takes away the record kept under the IMDb id of a video being
    /// marked unwatched on a title known by another id.
    func forgetLinked(kind: String, titleID: String, videoID: String) {
        guard !TMDB.isIMDbID(titleID), let ids = serviceIDs(kind: kind, titleID: titleID, videoID: videoID) else { return }
        if kind == "series" {
            try? store.removeVideoProgress(kind: kind, titleID: ids.titleID, videoID: ids.videoID)
        } else {
            for recorded in (try? store.finishedVideos(kind: kind, titleID: ids.titleID)) ?? [] {
                try? store.removeVideoProgress(kind: kind, titleID: ids.titleID, videoID: recorded)
            }
        }
    }

    /// Queues what was watched on titles known by a media server's id that
    /// the accounts do not have yet (after a full pull, everything they
    /// have is kept under the IMDb id).
    func sendMissingLinked() {
        let rows = ((try? store.allProgress()) ?? []).filter { $0.finished && !TMDB.isIMDbID($0.titleID) }
        let queued = (try? store.queuedChanges()) ?? []
        var links: [String: IMDbLink?] = [:]
        var missing: [WatchProgress] = []
        for row in rows {
            let key = row.kind + "\u{1}" + row.titleID
            if links[key] == nil { links[key] = .some(imdbLink(kind: row.kind, titleID: row.titleID)) }
            guard let link = links[key] ?? nil, let ids = link.serviceIDs(kind: row.kind, videoID: row.videoID) else { continue }
            let known = (try? store.progress(kind: row.kind, titleID: ids.titleID, videoID: ids.videoID))??.finished ?? false
            let waiting = queued.contains { $0.titleID == ids.titleID && $0.videoID == ids.videoID }
            if !known && !waiting { missing.append(row) }
        }
        for row in missing { queueSync("watched", row.kind, row.titleID, row.videoID) }
    }
}
