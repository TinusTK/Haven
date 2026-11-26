package net.haven.app.core

/** A change still to be sent to the linked accounts: "watched",
 *  "unwatched", "watchlist_add" or "watchlist_remove". */
data class Change(
    val id: Long = 0,
    val what: String,
    val kind: String,
    val titleId: String,
    val videoId: String,
    val createdAt: Long,
)

/** A film or episode saved to this device, or on its way. */
data class Download(
    /** Its number here; 0 before it is saved. */
    val id: Long = 0,
    /** "movie" or "series". */
    val kind: String,
    val titleId: String,
    /** The film's or episode's own id. */
    val videoId: String,
    val title: String,
    /** For an episode, which one ("S1 E2 · Name"); empty for a film. */
    val detail: String,
    val season: Int?,
    val episode: Int?,
    val poster: String?,
    /** Where it is fetched from. */
    val url: String,
    /** The file it is saved to. */
    val path: String,
    /** Its whole size in bytes; 0 while unknown. */
    val size: Long,
    /** Bytes saved so far. */
    val received: Long,
    /** "waiting", "loading", "done", "failed" or "paused". */
    val state: String,
    /** In milliseconds since 1970. */
    val addedAt: Long,
)

/** An installed add-on: its address and the manifest it served, raw JSON. */
data class StoredAddon(val url: String, val manifest: String)

/** How far one film or episode has been watched. */
data class WatchProgress(
    /** "movie", "series"… */
    val kind: String,
    /** The title's id (for an episode, its series). */
    val titleId: String,
    /** The film's or episode's own id; what streams are asked for. */
    val videoId: String,
    val title: String,
    /** For an episode, which one ("S1 E2 · Name"); empty for a film. */
    val detail: String,
    val poster: String?,
    /** Seconds from the start. */
    val position: Double,
    /** Running time in seconds. */
    val duration: Double,
    /** Watched to the end (or near enough). */
    val finished: Boolean,
    /** When this was saved, in milliseconds since 1970. */
    val updatedAt: Long,
)

/** A media server the user signed in to. The password is never kept. */
data class StoredServer(
    val id: Long = 0,
    /** "jellyfin", "emby" or "plex". */
    val kind: String,
    val name: String,
    val address: String,
    val userId: String,
    /** What the server gave in exchange for signing in. */
    val token: String,
) {
    /** Never shows the token. */
    override fun toString(): String = "StoredServer(id=$id, kind=$kind, name=$name, address=$address, ..)"
}

/** A source of television channels and radio stations: a playlist's
 *  address and the playlist as it was last fetched. */
data class TvSource(val url: String, val name: String, val playlist: String)

/** A title saved to watch later. */
data class Saved(
    val kind: String,
    val titleId: String,
    val title: String,
    /** Year or years, as given; may be empty. */
    val year: String,
    val poster: String?,
    /** When it was saved, in milliseconds since 1970. */
    val addedAt: Long,
)
