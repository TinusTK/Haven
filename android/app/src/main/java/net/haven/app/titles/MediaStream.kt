package net.haven.app.titles

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonPrimitive

/** How a stream is delivered. */
enum class StreamKind {
    /** A web address Haven's player can open. */
    Direct,
    /** A page to open in a web browser. */
    External,
    YouTube,
    /** A peer-to-peer link Haven cannot play. */
    PeerToPeer,
}

/** One way to watch a title. */
data class MediaStream(
    /** Short label, often the quality ("1080p"). */
    val name: String? = null,
    /** Longer text; `description` now, `title` before. */
    val title: String? = null,
    val description: String? = null,
    /** A direct address a player can open. */
    val url: String? = null,
    val externalUrl: String? = null,
    val ytId: String? = null,
    val infoHash: String? = null,
    /** The file's size in bytes, from the `videoSize` hint. */
    val videoSize: Long? = null,
    /** Who provided the stream, when whoever hands it on says so (a media
     *  server that gathers streams). Never read from the stream's own answer. */
    val provider: String? = null,
) {
    /** How the stream is delivered; null if it names no source at all. */
    val kind: StreamKind?
        get() = when {
            !url.isNullOrBlank() -> StreamKind.Direct
            !externalUrl.isNullOrBlank() -> StreamKind.External
            !ytId.isNullOrBlank() -> StreamKind.YouTube
            !infoHash.isNullOrBlank() -> StreamKind.PeerToPeer
            else -> null
        }

    /** The longer text, whichever field was used. */
    val details: String? get() = description ?: title

    companion object {
        fun from(o: JObj) = MediaStream(
            name = o.textOrNumber("name"), title = o.textOrNumber("title"), description = o.textOrNumber("description"),
            url = o.optionalString("url"), externalUrl = o.optionalString("externalUrl"),
            ytId = o.optionalString("ytId"), infoHash = o.optionalString("infoHash"),
            videoSize = size(JObj.of(o["behaviorHints"])?.get("videoSize")),
        )

        /** A size hint sent as a number or text; nothing for zero. */
        private fun size(value: Any?): Long? {
            val p = value as? JsonPrimitive ?: return null
            val bytes = when {
                p.isString -> p.content.trim().toLongOrNull()
                JObj.isBool(p) || JObj.isFloat(p) -> null
                else -> p.content.toLongOrNull()
            }
            return bytes?.takeIf { it > 0 }
        }

        /** Reads a `stream` answer. Entries that are not objects, or that name
         *  no source, are left out. */
        fun parseList(json: String): List<MediaStream> {
            val answer = Titles.top(json)
            val streams = answer["streams"] ?: return emptyList()
            val list = streams as? JsonArray ?: throw TitlesError.BadResponse("streams is not a list")
            return list.mapNotNull { item -> JObj.of(item)?.let { runCatching { from(it) }.getOrNull() } }
                .filter { it.kind != null }
        }
    }
}
