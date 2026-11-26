package net.haven.app.library

import net.haven.app.core.WatchProgress
import net.haven.app.metadata.Omdb
import net.haven.app.player.Resume
import net.haven.app.titles.MetaDetail
import net.haven.app.titles.Video
import java.math.BigDecimal
import java.math.RoundingMode

/** The top of Home: the banner's slides and the Continue Watching cards. */
object HomeTop {
    /** Titles the banner turns through. */
    const val SLIDES = 8
    /** Milliseconds each banner title shows. */
    const val TURN_EVERY_MILLIS = 9_000L
    /** Continue Watching lists this many titles. */
    const val CONTINUE_LENGTH = 20

    /** A score under the banner's title: "IMDb 8.4". `tone`: 0 IMDb (gold),
     *  1 TMDB (teal), 2 Rotten Tomatoes (red), 3 Metacritic (green). */
    data class ScorePill(val label: String, val value: String, val tone: Int)

    /** One title in the banner. */
    data class Slide(
        val id: String,
        val kind: String,
        val title: String,
        /** "2026  ·  Adventure, Action, Fantasy  ·  173 min  ·  R" */
        val facts: String,
        val description: String = "",
        val scores: List<ScorePill> = emptyList(),
        /** The title drawn as a picture, when there is one. */
        val logo: String? = null,
        /** The wide picture (else the poster). */
        val image: String? = null,
    )

    /** One card on Continue Watching. */
    data class ContinueCard(
        val id: String,
        val kind: String,
        /** The film or episode to carry on with. */
        val video: String,
        val title: String,
        /** The episode's name, or its "S03E06". */
        val episode: String,
        /** "S03E06"; empty for a film. */
        val badge: String,
        /** Aired episodes still to watch, from this one on (series). */
        val left: Int,
        val progress: Double,
        /** The wide picture (else the poster). */
        val picture: String?,
    )

    /** What is known of a title for Continue Watching. */
    data class Known(val background: String?, val videos: List<Video>)

    /** "S03E06" from an episode id such as `tt123:3:6`. */
    fun episodeBadge(videoId: String): String {
        val parts = videoId.split(":").reversed()
        if (parts.size < 2) return ""
        val episode = parts[0].toUIntOrNull() ?: return ""
        val season = parts[1].toUIntOrNull() ?: return ""
        return "S%02dE%02d".format(season.toLong(), episode.toLong())
    }

    /** How many aired episodes are still to watch, from `current` on (a
     *  series' regular episodes, in order; specials left out). */
    fun episodesLeft(videos: List<Video>, current: String, finished: Set<String>, today: String): Int {
        fun aired(v: Video): Boolean {
            if ((v.season ?: 1) <= 0 || v.episode == null) return false
            val released = v.released ?: return false
            return released.take(10) <= today
        }
        fun order(v: Video) = (v.season ?: 1) * 100_000 + (v.episode ?: 0)
        // sortedBy is stable: equal numbers keep their order.
        val episodes = videos.filter(::aired).sortedBy(::order)
        val at = episodes.indexOfFirst { it.id == current }
        if (at < 0) return 0
        return episodes.drop(at).count { it.id !in finished }
    }

    /** The line under the banner's title. */
    fun bannerFacts(meta: MetaDetail, scores: Omdb.Scores?): String {
        val parts = mutableListOf<String>()
        meta.releaseInfo?.takeIf { it.isNotEmpty() }?.let(parts::add)
        if (meta.genres.isNotEmpty()) parts += meta.genres.take(3).joinToString(", ")
        (meta.runtime ?: scores?.runtime)?.takeIf { it.isNotEmpty() }?.let(parts::add)
        scores?.rated?.let(parts::add)
        return parts.joinToString("  ·  ")
    }

    /** The scores under the banner's title, those that are known. */
    fun scorePills(meta: MetaDetail, tmdb: Double?, scores: Omdb.Scores?): List<ScorePill> = buildList {
        (meta.rating ?: scores?.imdb)?.takeIf { it.isNotEmpty() }?.let { add(ScorePill("IMDb", it, 0)) }
        if (tmdb != null && tmdb > 0) add(ScorePill("TMDB", BigDecimal(tmdb).setScale(1, RoundingMode.HALF_EVEN).toPlainString(), 1))
        scores?.rottenTomatoes?.let { add(ScorePill("RT", it, 2)) }
        scores?.metacritic?.let { add(ScorePill("MC", it, 3)) }
    }

    /** Continue Watching's cards from what was watched and what is known of
     *  each title (its wide picture and episodes). */
    fun cards(
        watched: List<WatchProgress>,
        known: Map<Releases.Title, Known>,
        finished: (WatchProgress) -> Set<String>,
        today: String,
    ): List<ContinueCard> = watched.map { p ->
        val key = Releases.Title(p.kind, p.titleId)
        val mark = p.detail.indexOf(" · ")
        val (name, badge) = when {
            mark >= 0 -> p.detail.substring(mark + 3) to episodeBadge(p.videoId)
            p.detail.isEmpty() -> "" to ""
            else -> "" to episodeBadge(p.videoId)
        }
        val left = if (p.kind == "series") known[key]?.let { episodesLeft(it.videos, p.videoId, finished(p), today) } ?: 0 else 0
        ContinueCard(
            p.titleId, p.kind, p.videoId, p.title, name.ifEmpty { badge }, badge, left,
            maxOf(Resume.share(p.position, p.duration).toDouble(), 0.02),
            known[key]?.background ?: p.poster,
        )
    }
}
