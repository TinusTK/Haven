package net.haven.app.library

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import net.haven.app.core.Http
import net.haven.app.core.Log
import net.haven.app.core.Saved
import net.haven.app.core.WatchProgress
import net.haven.app.metadata.Tmdb
import net.haven.app.metadata.WhereToWatch
import net.haven.app.pictures.Generation
import net.haven.app.player.Resume
import net.haven.app.titles.MetaDetail
import net.haven.app.titles.Video

/**
 * A title's page: its details (from where it came from, kept from last time,
 * filled in by TMDB), where to watch it, the Watchlist and watched marks, its
 * seasons and episodes, and the places to watch the film or an episode. The
 * Rust build's title page in `library.rs`, `marks.rs`, `offers.rs` and
 * `tracking.rs` (`enrich`).
 */
class TitlePage(private val library: Library, private val scope: CoroutineScope) {
    val sources = SourcesPanel(library, scope)

    // The title.
    var kind by mutableStateOf("")
        private set
    var id by mutableStateOf("")
        private set
    var meta by mutableStateOf<MetaDetail?>(null)
        private set
    var title by mutableStateOf("")
        private set
    /** Year, runtime, rating and genres on one line. */
    var facts by mutableStateOf("")
        private set
    var description by mutableStateOf("")
        private set
    var cast by mutableStateOf("")
        private set
    var director by mutableStateOf("")
        private set
    var poster by mutableStateOf<String?>(null)
        private set
    var background by mutableStateOf<String?>(null)
        private set
    var loading by mutableStateOf(false)
        private set
    var error by mutableStateOf("")
        private set
    /** On the Watchlist. */
    var saved by mutableStateOf(false)
        private set
    /** A film watched to the end. */
    var watched by mutableStateOf(false)
        private set
    /** A film (marked watched as a whole). */
    var film by mutableStateOf(false)
        private set

    // Where to watch.
    var offers by mutableStateOf(listOf<WhereToWatch.Tile>())
        private set
    var offersLink by mutableStateOf("")
        private set
    var offersNote by mutableStateOf("")
        private set

    // Seasons and episodes.
    var seasons by mutableStateOf(listOf<Int>())
        private set
    var season by mutableIntStateOf(0)
        private set
    var episodes by mutableStateOf(listOf<EpisodeRow>())
        private set
    var seasonAllWatched by mutableStateOf(false)
        private set
    /** Words that narrow the episode list. */
    var episodeFilter by mutableStateOf("")
        private set

    // The panel under the details: episodes, then places to watch.
    var panelOpen by mutableStateOf(false)
    var episodeOpen by mutableStateOf(false)
    var episodeLabel by mutableStateOf("")
        private set
    /** Said over the places to watch (the player, a download…). */
    var playNote by mutableStateOf("")
    /** Where the film or episode listed would resume. */
    var resumeNote by mutableStateOf("")
        private set

    /** The episode to pick once the details arrive (Continue Watching, Upcoming). */
    private var wantedVideo: String? = null
    private val generation = Generation()

    init {
        sources.playRow = { play(it) }
        sources.giveUp = { library.dismissPlayer() }
    }

    /** A series: it lists episodes before places to watch. */
    val isSeries get() = episodes.isNotEmpty() || seasons.isNotEmpty() || meta?.videos?.isNotEmpty() == true
    /** The panel shows places to watch (a film, or an episode once picked). */
    val sourcesShown get() = !isSeries || episodeOpen

    // Opening

    /** Opens a title's page and asks where it came from for its details. */
    fun open(opening: Opening) {
        val token = generation.next()
        sources.clear()
        kind = opening.kind
        id = opening.id
        meta = null
        title = opening.title
        facts = ""
        description = ""
        cast = ""
        director = ""
        poster = opening.poster
        background = null
        loading = true
        error = ""
        saved = runCatching { library.store.inWatchlist(kind, id) }.getOrDefault(false)
        watched = library.filmWatched(kind, id)
        film = kind == "movie"
        offers = emptyList()
        offersLink = ""
        offersNote = ""
        seasons = emptyList()
        season = 0
        episodes = emptyList()
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
        library.keptDetails(kind, id)?.let { loaded(token, Result.success(it)) }
        val fetch = library.detailsOf(kind, id)
        if (fetch == null) {
            val why = if (Tmdb.splitId(id) != null && library.tmdb == null) TitleRules.NO_TMDB_KEY else TitleRules.noDetails
            loaded(token, Result.failure(IllegalStateException(why)))
            return
        }
        scope.launch {
            val found = runCatching { fetch() }
            loaded(token, found.fold({ Result.success(it) }, { Result.failure(IllegalStateException("Could not load the details: ${Http.words(it.cause ?: it)}.")) }))
        }
    }

    fun close() {
        generation.next()
        sources.clear()
        meta = null
        id = ""
        wantedVideo = null
    }

    private fun loaded(token: Long, result: Result<MetaDetail>) {
        if (!generation.isCurrent(token)) return
        val shown = meta
        val fresh = result.getOrElse {
            loading = false
            // The kept details stay when fresh ones cannot be had.
            if (shown == null) error = it.message ?: ""
            return
        }
        if (fresh == shown) {
            loading = false // nothing new: the page stays as it is
            return
        }
        loading = false
        error = ""
        title = fresh.name
        facts = TitleRules.facts(fresh)
        description = (fresh.description ?: "").trim()
        cast = fresh.cast.take(TitleRules.CAST_SHOWN).joinToString(", ")
        director = fresh.director.joinToString(", ")
        poster = poster ?: fresh.poster
        fresh.background?.let { background = it }
        meta = fresh
        val seasonList = fresh.seasons

        // Fresh details for a page already showing: new episodes and text come
        // in, the season and episode picked stay as they are.
        if (shown != null) {
            seasonList.firstOrNull()?.let { first ->
                seasons = seasonList
                showSeason(if (season in seasonList) season else first)
            }
            return
        }
        enrich(token, fresh)
        loadOffers(token, fresh)
        if (fresh.videos.isEmpty()) {
            sources.say("")
            return
        }
        sources.say(TitleRules.CHOOSE_EPISODE)
        val wanted = wantedVideo?.let { want -> fresh.videos.firstOrNull { it.id == want } }
        wantedVideo = null
        val first = seasonList.firstOrNull()
        if (first != null) {
            seasons = seasonList
            showSeason(wanted?.season ?: first)
        } else {
            // Videos with no seasons (a channel's uploads): list them all.
            val finished = finishedHere()
            episodes = fresh.videos.map { TitleRules.episodeRow(it).copy(watched = it.id in finished) }
        }
        wanted?.let { openEpisode(it.id) }
    }

    /** Fills in what the details lack from TMDB, by the IMDb id. */
    private fun enrich(token: Long, meta: MetaDetail) {
        val tmdb = library.tmdb ?: return
        val imdb = listOfNotNull(meta.id, meta.imdbId).firstOrNull { Tmdb.isImdbId(it) } ?: return
        scope.launch {
            val details = runCatching { tmdb.details(meta.kind == "series", imdb) }
                .onFailure { Log.note("TMDB details: ${Http.words(it.cause ?: it)}") }.getOrNull() ?: return@launch
            if (!generation.isCurrent(token)) return@launch
            if (description.isEmpty()) description = details.overview ?: ""
            if (cast.isEmpty()) cast = details.cast.take(TitleRules.CAST_SHOWN).joinToString(", ")
            if (director.isEmpty()) director = details.directors.joinToString(", ")
            // A picture of the title's own wins.
            if (poster == null) poster = details.poster
            if (background == null) background = details.backdrop
        }
    }

    /** The services that carry the title, for the country in use. */
    private fun loadOffers(token: Long, meta: MetaDetail) {
        offers = emptyList()
        offersLink = ""
        val tmdb = library.tmdb
        if (tmdb == null) {
            offersNote = WhereToWatch.NO_KEY_NOTE
            return
        }
        offersNote = WhereToWatch.LOOKING_NOTE
        val chosen = WhereToWatch.country(library.store)
        scope.launch {
            val found = WhereToWatch.lookUp(meta, tmdb, chosen)
            if (!generation.isCurrent(token)) return@launch
            offers = found.tiles
            offersLink = found.link
            offersNote = found.note
        }
    }

    // Seasons and episodes

    fun showSeason(number: Int) {
        val meta = meta ?: return
        val words = episodeFilter.lowercase().trim()
        val picked = sources.listed?.videoId
        val finished = finishedHere()
        episodes = meta.episodes(maxOf(number, 0)).map {
            TitleRules.episodeRow(it).copy(selected = picked == it.id, watched = it.id in finished)
        }.filter { words.isEmpty() || it.title.lowercase().contains(words) || it.number.lowercase().contains(words) }
        season = number
        seasonAllWatched = episodes.isNotEmpty() && episodes.all { it.watched }
    }

    fun filterEpisodes(words: String) {
        if (words == episodeFilter) return
        episodeFilter = words
        showSeason(season)
    }

    /** A season chip was tapped: its episodes in the panel. */
    fun chooseSeason(number: Int) {
        episodeOpen = false
        panelOpen = true
        showSeason(number)
    }

    /** An episode was picked: where it can be watched. */
    fun openEpisode(videoId: String) {
        var heading = TitleRules.HEADING
        var detail = ""
        episodes = episodes.map { row ->
            if (row.id == videoId) {
                heading = "${TitleRules.HEADING} ${row.number} ${row.title}"
                val number = if (seasons.isNotEmpty() && row.number.isNotEmpty()) "S$season ${row.number}" else row.number
                detail = listOf(number, row.title).filter { it.isNotEmpty() }.joinToString(" · ")
            }
            row.copy(selected = row.id == videoId)
        }
        sources.load(asking(videoId), heading.trim())
        sources.setDetail(detail)
        episodeLabel = detail
        episodeOpen = true
        panelOpen = true
        showResumeNote()
    }

    /** A film's "Sources": looks up, now, where it can be watched. */
    fun showFilmSources() {
        panelOpen = true
        sources.load(asking(id), TitleRules.HEADING)
        showResumeNote()
    }

    private fun asking(videoId: String) = SourcesPanel.Asking(kind, id, videoId, meta, title)

    // Playing

    /** What playing a row asks for. */
    fun request(row: StreamRow): PlayRequest? {
        val address = row.address ?: return null
        val listed = sources.listed ?: return null
        val next = meta?.let { TitleRules.nextVideo(it, listed.videoId) }?.let(TitleRules::episodeDetail)
        return PlayRequest(address, title, listed.detail, kind, id, listed.videoId, meta?.poster ?: poster, row.source,
            next = next, serverItem = row.serverItem)
    }

    fun play(row: StreamRow) {
        val request = request(row) ?: return
        library.lastPlayed = row.source to row.name
        val play = library.play
        if (play != null) {
            playNote = ""
            play(request)
        } else {
            playNote = "Haven's player comes in the next part."
        }
    }

    /** The episode after the one listed. */
    val nextVideo: Video? get() {
        val meta = meta ?: return null
        val listed = sources.listed ?: return null
        return TitleRules.nextVideo(meta, listed.videoId)
    }

    /** Says where the listed film or episode would resume, if anywhere. */
    fun showResumeNote() {
        val listed = sources.listed
        val saved = listed?.let { runCatching { library.store.progress(kind, id, it.videoId) }.getOrNull() }
        resumeNote = when {
            saved == null -> ""
            saved.finished -> "You have watched this."
            else -> Resume.resumeFrom(saved.position, saved.duration, false)?.let {
                "Resumes at ${Resume.clock(it)} of ${Resume.clock(saved.duration)}."
            } ?: ""
        }
    }

    // Watchlist

    /** Puts the title on the Watchlist, or takes it off. */
    fun toggleWatchlist() {
        val wasSaved = saved
        val done = runCatching {
            if (wasSaved) library.store.removeFromWatchlist(kind, id)
            else library.store.addToWatchlist(Saved(kind, id, title, meta?.releaseInfo ?: "", meta?.poster ?: poster, System.currentTimeMillis()))
        }
        if (done.isFailure) {
            Log.note("could not change the watchlist: ${done.exceptionOrNull()?.message}")
            return
        }
        saved = !wasSaved
        library.reloadWatchlist()
        library.trackWatchlist(kind, id, !wasSaved)
    }

    // Watched marks

    /** The episodes of the open title watched to the end. */
    private fun finishedHere(): Set<String> = library.finishedOf(kind, id)

    /** Marks one film or episode, here and (by sync) on the accounts. */
    private fun markOne(videoId: String, detail: String, poster: String?, on: Boolean) {
        runCatching {
            if (on) {
                // Something partly watched becomes finished.
                library.store.saveProgress(
                    WatchProgress(kind, id, videoId, meta?.name ?: title, detail, poster, 0.0, 0.0, true, System.currentTimeMillis()),
                )
            } else {
                // A tick that came from a service under the IMDb id goes too.
                library.forgetLinked(kind, id, videoId)
                library.store.removeVideoProgress(kind, id, videoId)
            }
            library.queueSync(if (on) "watched" else "unwatched", kind, id, videoId)
        }.onFailure { Log.note("could not mark it ${if (on) "watched" else "unwatched"}: ${it.message}") }
    }

    /** Marks episodes of the open series by their ids. */
    fun markEpisodes(ids: List<String>, on: Boolean) {
        val meta = meta ?: return
        for (video in meta.videos) if (video.id in ids) markOne(video.id, TitleRules.episodeDetail(video), meta.poster, on)
        afterMarking()
    }

    /** Marks every episode of the season showing (unwatched: all of them;
     *  watched: those that have come out). */
    fun markSeason(on: Boolean) {
        val meta = meta ?: return
        val today = Day.at(java.time.Instant.ofEpochSecond(library.now()), library.zone).toString()
        markEpisodes(meta.episodes(maxOf(season, 0)).filter { !on || TitleRules.aired(it, today) }.map { it.id }, on)
    }

    /** Marks the open film. */
    fun markFilm(on: Boolean) {
        if (on) {
            markOne(id, "", meta?.poster ?: poster, true)
        } else {
            // Whatever was recorded for the film (its id may differ).
            for (video in library.finishedOf(kind, id)) markOne(video, "", null, false)
            runCatching { library.store.removeVideoProgress(kind, id, id) }
        }
        afterMarking()
    }

    /** Puts every tick, Continue Watching and the Watchlist right again. */
    private fun afterMarking() {
        refreshTicks()
        library.marked()
    }

    /** The ticks on the open title after something was watched or marked. */
    fun refreshTicks() {
        if (id.isEmpty()) return
        watched = library.filmWatched(kind, id)
        val finished = finishedHere()
        episodes = episodes.map { it.copy(watched = it.id in finished) }
        seasonAllWatched = episodes.isNotEmpty() && episodes.all { it.watched }
        showResumeNote()
    }
}
