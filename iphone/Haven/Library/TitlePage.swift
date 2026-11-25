import Foundation
import Observation

/// A title's page: its details (from where it came from, kept from last
/// time, filled in by TMDB), where to watch it, the Watchlist and watched
/// marks, its seasons and episodes, and the places to watch the film or an
/// episode. The Rust build's title page in `library.rs`, `marks.rs`,
/// `offers.rs` and `tracking.rs` (`enrich`).
@MainActor @Observable
final class TitlePage {
    @ObservationIgnored private weak var library: Library?
    let sources: SourcesPanel

    // The title.
    private(set) var kind = ""
    private(set) var id = ""
    private(set) var meta: MetaDetail?
    private(set) var title = ""
    /// Year, runtime, rating and genres on one line.
    private(set) var facts = ""
    private(set) var description = ""
    private(set) var cast = ""
    private(set) var director = ""
    private(set) var poster: String?
    private(set) var background: String?
    private(set) var loading = false
    private(set) var error = ""
    /// On the Watchlist.
    private(set) var saved = false
    /// A film watched to the end.
    private(set) var watched = false
    /// A film (marked watched as a whole).
    private(set) var film = false

    // Where to watch.
    private(set) var offers: [WhereToWatch.Tile] = []
    private(set) var offersLink = ""
    private(set) var offersNote = ""

    // Seasons and episodes.
    private(set) var seasons: [Int] = []
    private(set) var season = 0
    private(set) var episodes: [EpisodeRow] = []
    private(set) var seasonAllWatched = false
    /// Words that narrow the episode list.
    var episodeFilter = "" { didSet { if episodeFilter != oldValue { showSeason(season) } } }

    // The panel under the details: episodes, then places to watch.
    var panelOpen = false
    var episodeOpen = false
    private(set) var episodeLabel = ""
    /// Said over the places to watch (the player, a download…).
    var playNote = ""
    /// Where the film or episode listed would resume.
    private(set) var resumeNote = ""

    /// The episode to pick once the details arrive (Continue Watching, Upcoming).
    private var wantedVideo: String?
    private let generation = Generation()

    init(library: Library) {
        self.library = library
        sources = SourcesPanel(library: library)
        sources.playRow = { [weak self] in self?.play($0) }
        sources.giveUp = { [weak library] in library?.dismissPlayer() }
    }

    /// A series: it lists episodes before places to watch.
    var isSeries: Bool { !episodes.isEmpty || !seasons.isEmpty || !(meta?.videos.isEmpty ?? true) }
    /// The panel shows places to watch (a film, or an episode once picked).
    var sourcesShown: Bool { !isSeries || episodeOpen }

    // MARK: Opening

    /// Opens a title's page and asks where it came from for its details.
    func open(_ opening: Opening) {
        guard let library else { return }
        let token = generation.next()
        sources.clear()
        kind = opening.kind
        id = opening.id
        meta = nil
        title = opening.title
        facts = ""
        description = ""
        cast = ""
        director = ""
        poster = opening.poster
        background = nil
        loading = true
        error = ""
        saved = (try? library.store.inWatchlist(kind: kind, titleID: id)) ?? false
        watched = library.filmWatched(kind: kind, id: id)
        film = kind == "movie"
        offers = []
        offersLink = ""
        offersNote = ""
        seasons = []
        season = 0
        episodes = []
        seasonAllWatched = false
        episodeFilter = ""
        panelOpen = false
        episodeOpen = false
        episodeLabel = ""
        playNote = ""
        resumeNote = ""
        // A Continue Watching card names the episode to go back to.
        wantedVideo = opening.video

        // Shown as it was last time straight away; the fresh details only
        // change the page if something is new.
        if let kept = library.keptDetails(kind: kind, id: id) {
            loaded(token, .success(kept))
        }
        guard let fetch = library.detailsOf(kind: kind, id: id) else {
            let why = TMDB.splitID(id) != nil && library.tmdb == nil ? TitleRules.noTMDBKey : TitleRules.noDetails
            loaded(token, .failure(Problem(why)))
            return
        }
        Task {
            do {
                loaded(token, .success(try await fetch()))
            } catch {
                loaded(token, .failure(Problem("Could not load the details: \(HTTP.words(error)).")))
            }
        }
    }

    func close() {
        generation.next()
        sources.clear()
        meta = nil
        wantedVideo = nil
    }

    func loaded(_ token: UInt64, _ result: Result<MetaDetail, Problem>) {
        guard generation.isCurrent(token) else { return }
        let shown = meta
        let fresh: MetaDetail
        switch result {
        case let .success(found) where found == shown:
            loading = false // nothing new: the page stays as it is
            return
        case let .success(found):
            fresh = found
        case .failure where shown != nil:
            loading = false // the kept details stay when fresh ones cannot be had
            return
        case let .failure(problem):
            loading = false
            error = problem.words
            return
        }
        loading = false
        error = ""
        title = fresh.name
        facts = TitleRules.facts(fresh)
        description = (fresh.description ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        cast = fresh.cast.prefix(TitleRules.castShown).joined(separator: ", ")
        director = fresh.director.joined(separator: ", ")
        poster = poster ?? fresh.poster
        if let wide = fresh.background { background = wide }
        meta = fresh
        let seasonList = fresh.seasons

        // Fresh details for a page already showing: new episodes and text
        // come in, the season and episode picked stay as they are.
        if shown != nil {
            if let first = seasonList.first {
                seasons = seasonList
                showSeason(seasonList.contains(season) ? season : first)
            }
            return
        }
        enrich(token, fresh)
        loadOffers(token, fresh)
        guard !fresh.videos.isEmpty else {
            sources.say("")
            return
        }
        sources.say(TitleRules.chooseEpisode)
        let wanted = wantedVideo.flatMap { want in fresh.videos.first { $0.id == want } }
        wantedVideo = nil
        if let first = seasonList.first {
            seasons = seasonList
            showSeason(wanted?.season ?? first)
        } else {
            // Videos with no seasons (a channel's uploads): list them all.
            let finished = finishedHere()
            episodes = fresh.videos.map { video in
                var row = TitleRules.episodeRow(video)
                row.watched = finished.contains(video.id)
                return row
            }
        }
        if let wanted { openEpisode(wanted.id) }
    }

    /// Fills in what the details lack from TMDB, by the IMDb id.
    private func enrich(_ token: UInt64, _ meta: MetaDetail) {
        guard let tmdb = library?.tmdb,
              let imdb = [meta.id, meta.imdbID].compactMap({ $0 }).first(where: TMDB.isIMDbID) else { return }
        let series = meta.kind == "series"
        Task {
            let details: TMDBDetails
            do {
                guard let found = try await tmdb.details(series: series, imdbID: imdb) else { return }
                details = found
            } catch {
                Log.note("TMDB details: \(HTTP.words(error))")
                return
            }
            guard generation.isCurrent(token) else { return }
            if description.isEmpty { description = details.overview ?? "" }
            if cast.isEmpty { cast = details.cast.prefix(TitleRules.castShown).joined(separator: ", ") }
            if director.isEmpty { director = details.directors.joined(separator: ", ") }
            // A picture of the title's own wins.
            if poster == nil { poster = details.poster }
            if background == nil { background = details.backdrop }
        }
    }

    /// The services that carry the title, for the country in use.
    private func loadOffers(_ token: UInt64, _ meta: MetaDetail) {
        guard let library else { return }
        offers = []
        offersLink = ""
        guard let tmdb = library.tmdb else {
            offersNote = WhereToWatch.noKeyNote
            return
        }
        offersNote = WhereToWatch.lookingNote
        let chosen = WhereToWatch.country(library.store)
        Task {
            let found = await WhereToWatch.lookUp(meta, tmdb: tmdb, chosen: chosen)
            guard generation.isCurrent(token) else { return }
            offers = found.tiles
            offersLink = found.link
            offersNote = found.note
        }
    }

    // MARK: Seasons and episodes

    func showSeason(_ number: Int) {
        guard let meta else { return }
        let words = episodeFilter.lowercased().trimmingCharacters(in: .whitespaces)
        let picked = sources.listed?.videoID
        let finished = finishedHere()
        episodes = meta.episodes(season: max(number, 0)).map { video in
            var row = TitleRules.episodeRow(video)
            row.selected = picked == video.id
            row.watched = finished.contains(video.id)
            return row
        }
        .filter { words.isEmpty || $0.title.lowercased().contains(words) || $0.number.lowercased().contains(words) }
        season = number
        seasonAllWatched = !episodes.isEmpty && episodes.allSatisfy(\.watched)
    }

    /// A season chip was tapped: its episodes in the panel.
    func chooseSeason(_ number: Int) {
        episodeOpen = false
        panelOpen = true
        showSeason(number)
    }

    /// An episode was picked: where it can be watched.
    func openEpisode(_ videoID: String) {
        var heading = TitleRules.heading
        var detail = ""
        for at in episodes.indices {
            let row = episodes[at]
            if row.id == videoID {
                heading = "\(TitleRules.heading) \(row.number) \(row.title)"
                let number = !seasons.isEmpty && !row.number.isEmpty ? "S\(season) \(row.number)" : row.number
                detail = [number, row.title].filter { !$0.isEmpty }.joined(separator: " · ")
            }
            episodes[at].selected = row.id == videoID
        }
        sources.load(asking(videoID), heading: heading.trimmingCharacters(in: .whitespaces))
        sources.setDetail(detail)
        episodeLabel = detail
        episodeOpen = true
        panelOpen = true
        showResumeNote()
    }

    /// A film's "Sources": looks up, now, where it can be watched.
    func showFilmSources() {
        panelOpen = true
        sources.load(asking(id), heading: TitleRules.heading)
        showResumeNote()
    }

    private func asking(_ videoID: String) -> SourcesPanel.Asking {
        SourcesPanel.Asking(kind: kind, titleID: id, videoID: videoID, meta: meta, name: title)
    }

    // MARK: Playing

    /// What playing a row asks for.
    func request(_ row: StreamRow) -> PlayRequest? {
        guard let address = row.address, let listed = sources.listed else { return nil }
        let next = meta.flatMap { TitleRules.nextVideo($0, after: listed.videoID) }.map(TitleRules.episodeDetail)
        return PlayRequest(address: address, title: title, detail: listed.detail, kind: kind, titleID: id,
                           videoID: listed.videoID, poster: meta?.poster ?? poster, source: row.source, next: next,
                           serverItem: row.serverItem)
    }

    func play(_ row: StreamRow) {
        guard let request = request(row) else { return }
        library?.lastPlayed = (row.source, row.name)
        if let play = library?.play {
            playNote = ""
            play(request)
        } else {
            playNote = "Haven's player comes in the next part."
        }
    }

    /// The episode after the one listed.
    var nextVideo: Video? {
        guard let meta, let listed = sources.listed else { return nil }
        return TitleRules.nextVideo(meta, after: listed.videoID)
    }

    /// Says where the listed film or episode would resume, if anywhere.
    func showResumeNote() {
        guard let library, let listed = sources.listed,
              let saved = try? library.store.progress(kind: kind, titleID: id, videoID: listed.videoID) else {
            resumeNote = ""
            return
        }
        if saved.finished {
            resumeNote = "You have watched this."
        } else if let at = Resume.from(position: saved.position, duration: saved.duration, finished: false) {
            resumeNote = "Resumes at \(clockText(at)) of \(clockText(saved.duration))."
        } else {
            resumeNote = ""
        }
    }

    // MARK: Watchlist

    /// Puts the title on the Watchlist, or takes it off.
    func toggleWatchlist() {
        guard let library else { return }
        let wasSaved = saved
        do {
            if wasSaved {
                try library.store.removeFromWatchlist(kind: kind, titleID: id)
            } else {
                try library.store.addToWatchlist(Saved(kind: kind, titleID: id, title: title, year: meta?.releaseInfo ?? "",
                                                       poster: meta?.poster ?? poster, addedAt: nowMillis()))
            }
        } catch {
            Log.note("could not change the watchlist: \(error.localizedDescription)")
            return
        }
        saved = !wasSaved
        library.reloadWatchlist()
        library.trackWatchlist(kind, id, !wasSaved)
    }

    // MARK: Watched marks

    /// The episodes of the open title watched to the end.
    private func finishedHere() -> Set<String> {
        Set((try? library?.store.finishedVideos(kind: kind, titleID: id)) ?? [])
    }

    /// Marks one film or episode, here and (by sync) on the accounts.
    private func markOne(videoID: String, detail: String, poster: String?, on: Bool) {
        guard let library else { return }
        do {
            if on {
                // Something partly watched becomes finished.
                try library.store.saveProgress(WatchProgress(kind: kind, titleID: id, videoID: videoID, title: meta?.name ?? title,
                                                             detail: detail, poster: poster, position: 0, duration: 0,
                                                             finished: true, updatedAt: nowMillis()))
            } else {
                try library.store.removeVideoProgress(kind: kind, titleID: id, videoID: videoID)
            }
            library.queueSync(on ? "watched" : "unwatched", kind, id, videoID)
        } catch {
            Log.note("could not mark it \(on ? "watched" : "unwatched"): \(error.localizedDescription)")
        }
    }

    /// Marks episodes of the open series by their ids.
    func markEpisodes(_ ids: [String], on: Bool) {
        guard let meta else { return }
        for video in meta.videos where ids.contains(video.id) {
            markOne(videoID: video.id, detail: TitleRules.episodeDetail(video), poster: meta.poster, on: on)
        }
        afterMarking()
    }

    /// Marks every episode of the season showing (unwatched: all of them;
    /// watched: those that have come out).
    func markSeason(_ on: Bool) {
        guard let meta, let library else { return }
        let today = Day.today(library.zone).description
        let ids = meta.episodes(season: max(season, 0)).filter { !on || TitleRules.aired($0, today: today) }.map(\.id)
        markEpisodes(ids, on: on)
    }

    /// Marks the open film.
    func markFilm(_ on: Bool) {
        guard let library else { return }
        if on {
            markOne(videoID: id, detail: "", poster: meta?.poster ?? poster, on: true)
        } else {
            // Whatever was recorded for the film (its id may differ).
            for video in (try? library.store.finishedVideos(kind: kind, titleID: id)) ?? [] {
                markOne(videoID: video, detail: "", poster: nil, on: false)
            }
            try? library.store.removeVideoProgress(kind: kind, titleID: id, videoID: id)
        }
        afterMarking()
    }

    /// Puts every tick, Continue Watching and the Watchlist right again.
    private func afterMarking() {
        refreshTicks()
        library?.marked()
    }

    /// The ticks on the open title after something was watched or marked.
    func refreshTicks() {
        guard let library, !id.isEmpty else { return }
        watched = library.filmWatched(kind: kind, id: id)
        let finished = finishedHere()
        for at in episodes.indices { episodes[at].watched = finished.contains(episodes[at].id) }
        seasonAllWatched = !episodes.isEmpty && episodes.allSatisfy(\.watched)
        showResumeNote()
    }
}
