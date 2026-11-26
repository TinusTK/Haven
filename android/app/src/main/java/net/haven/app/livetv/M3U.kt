package net.haven.app.livetv

// Live TV and radio: reading channel lists. A source is the address of an
// M3U playlist (the usual "M3U Plus" kind: one `#EXTINF` line of details,
// then the channel's address). Entries marked `radio="true"` are radio
// stations; everything else is television. An Xtream Codes account is the
// other kind of source ([XtreamAccount]), turned into the same text.

/** One channel or station from a playlist. */
data class Channel(
    val name: String,
    /** Where it plays from. */
    val address: String,
    /** `group-title`, or the older `#EXTGRP` line. */
    val group: String? = null,
    /** Address of its logo (`tvg-logo`). */
    val logo: String? = null,
    /** Its id in a programme guide (`tvg-id`). */
    val guideId: String? = null,
    /** Marked `radio="true"`. */
    val radio: Boolean = false,
)

object M3U {
    /** Reads an M3U playlist. Lenient: a missing `#EXTM3U` header, blank lines,
     *  Windows line ends and unknown `#` lines are all fine; an entry without
     *  an address is left out, and an address without an `#EXTINF` line is
     *  named after its last path segment. */
    fun parse(text: String): List<Channel> {
        val channels = mutableListOf<Channel>()
        var pending: Channel? = null
        // Some files start with a byte-order mark.
        val body = text.trimStart('﻿')
        for (raw in body.split("\r\n", "\n", "\r")) {
            val line = raw.trim()
            if (line.isEmpty()) continue
            val info = stripPrefix(line, "#EXTINF:")
            val group = stripPrefix(line, "#EXTGRP:")
            when {
                info != null -> pending = readInfo(info)
                group != null -> if (pending != null && pending.group == null) pending = pending.copy(group = textOf(group))
                line.startsWith("#") -> Unit // #EXTM3U, #EXTVLCOPT and anything else: not needed.
                else -> {
                    var channel = (pending ?: Channel("", "")).copy(address = line)
                    pending = null
                    if (channel.name.isEmpty()) channel = channel.copy(name = nameFromAddress(line))
                    channels += channel
                }
            }
        }
        return channels
    }

    /** The rest of `line` after `prefix`, in any case. */
    private fun stripPrefix(line: String, prefix: String): String? =
        if (line.length >= prefix.length && line.substring(0, prefix.length).equals(prefix, ignoreCase = true)) line.substring(prefix.length) else null

    private fun textOf(text: String): String? = text.trim().takeIf { it.isNotEmpty() }

    private fun nameFromAddress(address: String): String {
        val path = address.split('?', '#').first()
        val last = path.trimEnd('/').split('/').last()
        return last.ifEmpty { address }
    }

    /** Reads what follows `#EXTINF:`: a duration, `key="value"` pairs, then a
     *  comma and the channel's name. */
    private fun readInfo(info: String): Channel {
        // The name starts after the first comma that is not inside quotes.
        var inQuotes = false
        var split = info.length
        for ((index, c) in info.withIndex()) {
            if (c == '"') {
                inQuotes = !inQuotes
            } else if (c == ',' && !inQuotes) {
                split = index
                break
            }
        }
        val details = info.substring(0, split)
        val name = info.substring(split).removePrefix(",")

        fun attribute(key: String): String? {
            var from = 0
            while (true) {
                val found = details.indexOf(key, from, ignoreCase = true).takeIf { it >= 0 } ?: return null
                val beforeOk = found == 0 || details[found - 1].isWhitespace()
                val after = details.substring(found + key.length)
                if (beforeOk && after.startsWith("=\"")) return textOf(after.substring(2).substringBefore('"'))
                from = found + key.length
            }
        }

        return Channel(
            name = textOf(name) ?: attribute("tvg-name") ?: "",
            address = "",
            // Several groups may be listed with ';': the first is used.
            group = attribute("group-title")?.let { textOf(it.split(';').first()) },
            logo = attribute("tvg-logo"),
            guideId = attribute("tvg-id"),
            radio = attribute("radio")?.equals("true", ignoreCase = true) == true,
        )
    }

    /** Writes channels as playlist text that [parse] reads back the same. */
    fun write(channels: List<Channel>): String {
        // Quotes and line ends would break a line apart; a `;` splits groups.
        fun value(text: String) = text.map { if (it == '"' || Character.isISOControl(it)) ' ' else it }.joinToString("").trim()
        val out = StringBuilder("#EXTM3U\n")
        for (channel in channels) {
            out.append("#EXTINF:-1")
            channel.guideId?.let { out.append(" tvg-id=\"${value(it)}\"") }
            channel.logo?.let { out.append(" tvg-logo=\"${value(it)}\"") }
            channel.group?.let { out.append(" group-title=\"${value(it).replace(";", ",")}\"") }
            if (channel.radio) out.append(" radio=\"true\"")
            val name = channel.name.map { if (Character.isISOControl(it)) ' ' else it }.joinToString("").trim()
            out.append(",$name\n${channel.address.trim()}\n")
        }
        return out.toString()
    }

    /** The channels' groups, in the order they first appear, without repeats. */
    fun groups(channels: List<Channel>): List<String> = channels.mapNotNull { it.group }.distinct()

    /** Whether a channel passes the filter: in `group` (when one is chosen) and
     *  with every word of `words` somewhere in its name, in any case. */
    fun matches(channel: Channel, group: String?, words: String): Boolean {
        if (group != null && channel.group != group) return false
        val name = channel.name.lowercase()
        return words.split(Regex("\\s+")).filter { it.isNotEmpty() }.all { name.contains(it.lowercase()) }
    }

    /** A short name for a source, from its address: the host. */
    fun sourceName(address: String): String {
        val rest = address.indexOf("://").takeIf { it >= 0 }?.let { address.substring(it + 3) } ?: address
        val host = rest.split('/', '?', '#').first()
        // Drop "user:password@" and the port.
        val bare = host.substringAfterLast('@')
        val name = if (bare.startsWith("[")) {
            bare.indexOf(']').takeIf { it >= 0 }?.let { bare.substring(0, it + 1) } ?: bare
        } else {
            bare.split(':').first()
        }
        return name.ifEmpty { address }
    }
}
