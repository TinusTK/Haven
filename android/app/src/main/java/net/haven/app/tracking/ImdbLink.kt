package net.haven.app.tracking

import net.haven.app.metadata.Tmdb
import net.haven.app.servers.Servers
import net.haven.app.titles.MetaDetail

/** A title Haven knows by another id than the IMDb one (a media server's
 *  own, say) linked to its IMDb id, which is how Trakt and Simkl know it.
 *  With the link a film or episode watched on a service shows as watched on
 *  the server's title, and one watched on the server's title is sent to the
 *  services. */
data class ImdbLink(
    val imdb: String,
    /** Episode ids against their season and episode numbers. */
    val episodes: Map<String, Pair<Int, Int>>,
) {
    /** A title and video as the services know them; null when an episode's numbers are not known. */
    fun serviceIds(kind: String, videoId: String): Pair<String, String>? {
        if (videoId.isEmpty()) return imdb to ""
        if (kind != "series") return imdb to imdb
        val numbers = episodes[videoId] ?: return null
        return imdb to episodeId(imdb, numbers)
    }

    /** Adds to `own` (a title's finished videos) those finished under the IMDb
     *  id (`elsewhere`), in the title's own ids. */
    fun merged(kind: String, titleId: String, own: Set<String>, elsewhere: Set<String>): Set<String> {
        val done = own.toMutableSet()
        if (kind == "series") {
            for ((id, numbers) in episodes) if (episodeId(imdb, numbers) in elsewhere) done += id
        } else if (elsewhere.isNotEmpty()) {
            done += titleId
        }
        return done
    }

    companion object {
        /** The IMDb form of an episode's id, as Haven keeps what services send. */
        fun episodeId(imdb: String, numbers: Pair<Int, Int>) = "$imdb:${numbers.first}:${numbers.second}"

        /** The link from a title's details (or a gathered server id). */
        fun of(titleId: String, detail: MetaDetail?): ImdbLink? {
            if (Tmdb.isImdbId(titleId)) return null
            val fromDetail = detail?.imdbId?.takeIf { Tmdb.isImdbId(it) }
            val fromId = Servers.splitId(titleId)?.let { Servers.imdbIn(it.second) }
            val imdb = fromDetail ?: fromId ?: return null
            val episodes = detail?.videos?.mapNotNull { v -> if (v.season != null && v.episode != null) v.id to (v.season to v.episode) else null }?.toMap()
                ?: emptyMap()
            return ImdbLink(imdb, episodes)
        }
    }
}
