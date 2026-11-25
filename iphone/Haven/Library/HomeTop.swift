import Foundation

/// The top of Home: the banner's slides and the Continue Watching cards.
enum HomeTop {
    /// Titles the banner turns through.
    static let slides = 8
    /// Seconds each banner title shows.
    static let turnEvery: Double = 9
    /// Continue Watching lists this many titles.
    static let continueLength = 20

    /// A score under the banner's title: "IMDb 8.4".
    struct ScorePill: Equatable, Sendable, Identifiable {
        var label: String
        var value: String
        /// 0 IMDb (gold), 1 TMDB (teal), 2 Rotten Tomatoes (red), 3 Metacritic (green).
        var tone: Int
        var id: String { label }
    }

    /// One title in the banner.
    struct Slide: Equatable, Sendable, Identifiable {
        var id: String
        var kind: String
        var title: String
        /// "2026  ·  Adventure, Action, Fantasy  ·  173 min  ·  R"
        var facts: String
        var description = ""
        var scores: [ScorePill] = []
        /// The title drawn as a picture, when there is one.
        var logo: String?
        /// The wide picture (else the poster).
        var image: String?
    }

    /// One card on Continue Watching.
    struct ContinueCard: Equatable, Sendable, Identifiable {
        var id: String
        var kind: String
        /// The film or episode to carry on with.
        var video: String
        var title: String
        /// The episode's name, or its "S03E06".
        var episode: String
        /// "S03E06"; empty for a film.
        var badge: String
        /// Aired episodes still to watch, from this one on (series).
        var left: Int
        var progress: Double
        /// The wide picture (else the poster).
        var picture: String?
    }

    /// "S03E06" from an episode id such as `tt123:3:6`.
    static func episodeBadge(_ videoID: String) -> String {
        let parts = videoID.split(separator: ":", omittingEmptySubsequences: false).reversed().map(String.init)
        guard parts.count >= 2, let episode = UInt32(parts[0]), let season = UInt32(parts[1]) else { return "" }
        return String(format: "S%02dE%02d", season, episode)
    }

    /// How many aired episodes are still to watch, from `current` on (a
    /// series' regular episodes, in order; specials left out).
    static func episodesLeft(_ videos: [Video], current: String, finished: Set<String>, today: String) -> Int {
        func aired(_ video: Video) -> Bool {
            guard (video.season ?? 1) > 0, video.episode != nil, let released = video.released else { return false }
            return String(released.prefix(10)) <= today
        }
        func order(_ video: Video) -> Int { (video.season ?? 1) * 100_000 + (video.episode ?? 0) }
        // Stable: equal numbers keep their order.
        let listed = videos.filter(aired).enumerated().map { (place: $0.offset, video: $0.element) }
        let episodes = listed
            .sorted { order($0.video) != order($1.video) ? order($0.video) < order($1.video) : $0.place < $1.place }
            .map(\.video)
        guard let at = episodes.firstIndex(where: { $0.id == current }) else { return 0 }
        return episodes[at...].filter { !finished.contains($0.id) }.count
    }

    /// The line under the banner's title.
    static func bannerFacts(_ meta: MetaDetail, scores: OMDb.Scores?) -> String {
        var parts: [String] = []
        if let year = meta.releaseInfo, !year.isEmpty { parts.append(year) }
        if !meta.genres.isEmpty { parts.append(meta.genres.prefix(3).joined(separator: ", ")) }
        if let runtime = meta.runtime ?? scores?.runtime, !runtime.isEmpty { parts.append(runtime) }
        if let rated = scores?.rated { parts.append(rated) }
        return parts.joined(separator: "  ·  ")
    }

    /// The scores under the banner's title, those that are known.
    static func scorePills(_ meta: MetaDetail, tmdb: Double?, scores: OMDb.Scores?) -> [ScorePill] {
        var pills: [ScorePill] = []
        if let imdb = meta.rating ?? scores?.imdb, !imdb.isEmpty { pills.append(ScorePill(label: "IMDb", value: imdb, tone: 0)) }
        if let tmdb, tmdb > 0 { pills.append(ScorePill(label: "TMDB", value: String(format: "%.1f", tmdb), tone: 1)) }
        if let rt = scores?.rottenTomatoes { pills.append(ScorePill(label: "RT", value: rt, tone: 2)) }
        if let mc = scores?.metacritic { pills.append(ScorePill(label: "MC", value: mc, tone: 3)) }
        return pills
    }

    /// Continue Watching's cards from what was watched and what is known of
    /// each title (its wide picture and episodes).
    static func cards(_ watched: [WatchProgress], known: [Releases.Title: (background: String?, videos: [Video])],
                      finished: (WatchProgress) -> Set<String>, today: String) -> [ContinueCard] {
        watched.map { progress in
            let key = Releases.Title(kind: progress.kind, id: progress.titleID)
            let name: String
            let badge: String
            if let mark = progress.detail.range(of: " · ") {
                name = String(progress.detail[mark.upperBound...])
                badge = episodeBadge(progress.videoID)
            } else if progress.detail.isEmpty {
                name = ""
                badge = ""
            } else {
                name = ""
                badge = episodeBadge(progress.videoID)
            }
            var left = 0
            if progress.kind == "series", let meta = known[key] {
                left = episodesLeft(meta.videos, current: progress.videoID, finished: finished(progress), today: today)
            }
            return ContinueCard(id: progress.titleID, kind: progress.kind, video: progress.videoID, title: progress.title,
                                episode: name.isEmpty ? badge : name, badge: badge, left: left,
                                progress: max(Resume.share(position: progress.position, duration: progress.duration), 0.02),
                                picture: known[key]?.background ?? progress.poster)
        }
    }
}
