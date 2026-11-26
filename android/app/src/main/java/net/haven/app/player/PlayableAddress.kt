package net.haven.app.player

/** Which addresses the player may open: network addresses only, never a
 *  local file or one of mpv's own sources (Swift `PlayableAddress`). */
object PlayableAddress {
    /** The address, trimmed, if it is an http or https address. */
    fun web(address: String): String? = network(address, listOf("http", "https"))

    /** Like [web], and also the streaming kinds television playlists use. */
    fun stream(address: String): String? =
        network(address, listOf("http", "https", "rtsp", "rtsps", "rtmp", "rtmps", "rtp", "udp", "mms", "mmsh"))

    private fun network(raw: String, schemes: List<String>): String? {
        val address = raw.trim()
        val mark = address.indexOf("://").takeIf { it >= 0 } ?: return null
        val scheme = address.substring(0, mark).lowercase()
        if (scheme !in schemes) return null
        var rest = address.substring(mark + 3)
        // "@" starts a multicast address, as in udp://@239.0.0.1:1234.
        if ((scheme == "udp" || scheme == "rtp") && rest.startsWith("@")) rest = rest.drop(1)
        val first = rest.firstOrNull() ?: return null
        if (!((first.code < 128 && first.isLetterOrDigit()) || first == '[')) return null
        val clean = address.none { Character.isISOControl(it) || Character.isWhitespace(it) || Character.isSpaceChar(it) }
        return if (clean) address else null
    }
}
