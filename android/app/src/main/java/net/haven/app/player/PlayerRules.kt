package net.haven.app.player

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.intOrNull
import net.haven.app.core.Store
import java.math.BigDecimal
import java.math.RoundingMode
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter

/** One audio or subtitle track of what is playing. `id` is mpv's number for
 *  the track within its kind; what `aid`/`sid` take. */
data class Track(val id: Int, val kind: Kind, val label: String, val selected: Boolean) {
    enum class Kind { Audio, Subtitle }
}

/** The player's rules, ported from the Rust build (`haven-player/src/
 *  tracks.rs`, `haven-app/src/player.rs`, `external.rs`). */
object PlayerRules {
    /** How often the place is saved while playing. */
    const val SAVE_EVERY_MILLIS = 5_000L
    /** After a seek the bar is left alone this long, so it does not jump back
     *  while mpv catches up. */
    const val SEEK_HOLD_MILLIS = 700L
    /** The bar of controls at the bottom, plus a little air: subtitles are
     *  lifted clear of it. */
    const val CONTROLS_HEIGHT = 112.0
    /** mpv's own subtitle margin, used while the controls are hidden. */
    const val SUBTITLE_MARGIN = 22
    /** How long the next episode waits before it starts by itself. */
    const val NEXT_AFTER_MILLIS = 10_000L
    /** The controls hide after this long untouched. */
    const val HIDE_AFTER_MILLIS = 3_000L
    val speeds = listOf(0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0)
    val fits = listOf("Whole picture", "Fill the screen (edges cut off)", "Stretch to the screen")

    // Tracks

    /** Reads mpv's `track-list`. Video tracks and entries that cannot be read
     *  are left out; text that is not a track list gives no tracks. */
    fun tracks(json: String): List<Track> {
        val entries = runCatching { Json.parseToJsonElement(json) as? JsonArray }.getOrNull() ?: return emptyList()
        return entries.mapNotNull { entry ->
            val raw = entry as? JsonObject ?: return@mapNotNull null
            val id = (raw["id"] as? JsonPrimitive)?.intOrNull ?: return@mapNotNull null
            val kind = when ((raw["type"] as? JsonPrimitive)?.takeIf { it.isString }?.content) {
                "audio" -> Track.Kind.Audio
                "sub" -> Track.Kind.Subtitle
                else -> return@mapNotNull null
            }
            Track(id, kind, label(raw, id, kind), (raw["selected"] as? JsonPrimitive)?.booleanOrNull ?: false)
        }
    }

    private fun label(raw: JsonObject, id: Int, kind: Track.Kind): String {
        fun text(key: String) = (raw[key] as? JsonPrimitive)?.takeIf { it.isString }?.content?.trim()?.takeIf { it.isNotEmpty() }
        val language = text("lang")?.takeIf { it != "und" }?.let(::languageName)
        val parts = mutableListOf<String>()
        language?.let(parts::add)
        // Many files repeat the language as the title.
        text("title")?.let { title -> if (language == null || !language.equals(title, ignoreCase = true)) parts += title }
        if (parts.isEmpty()) parts += "Track $id"
        if (kind == Track.Kind.Audio) {
            val codec = text("codec")?.uppercase()
            val layout = (raw["demux-channel-count"] as? JsonPrimitive)?.intOrNull?.let(::channelLayout)
            val sound = listOfNotNull(codec, layout)
            if (sound.isNotEmpty()) parts += sound.joinToString(" ")
        }
        if ((raw["forced"] as? JsonPrimitive)?.booleanOrNull == true) parts += "Forced"
        return parts.joinToString(" · ")
    }

    private fun channelLayout(channels: Int): String? = when {
        channels == 1 -> "Mono"
        channels == 2 -> "Stereo"
        channels == 6 -> "5.1"
        channels == 8 -> "7.1"
        channels > 0 -> "$channels ch"
        else -> null
    }

    private val languages: Map<String, String> = listOf(
        listOf("en", "eng") to "English", listOf("af", "afr") to "Afrikaans", listOf("ar", "ara") to "Arabic",
        listOf("bg", "bul") to "Bulgarian", listOf("ca", "cat") to "Catalan", listOf("cs", "ces", "cze") to "Czech",
        listOf("da", "dan") to "Danish", listOf("de", "deu", "ger") to "German", listOf("el", "ell", "gre") to "Greek",
        listOf("es", "spa") to "Spanish", listOf("et", "est") to "Estonian", listOf("fa", "fas", "per") to "Persian",
        listOf("fi", "fin") to "Finnish", listOf("fr", "fra", "fre") to "French", listOf("he", "heb") to "Hebrew",
        listOf("hi", "hin") to "Hindi", listOf("hr", "hrv") to "Croatian", listOf("hu", "hun") to "Hungarian",
        listOf("id", "ind") to "Indonesian", listOf("is", "isl", "ice") to "Icelandic", listOf("it", "ita") to "Italian",
        listOf("ja", "jpn") to "Japanese", listOf("ko", "kor") to "Korean", listOf("lt", "lit") to "Lithuanian",
        listOf("lv", "lav") to "Latvian", listOf("ms", "msa", "may") to "Malay", listOf("nb", "nob", "no", "nor") to "Norwegian",
        listOf("nl", "nld", "dut") to "Dutch", listOf("pl", "pol") to "Polish", listOf("pt", "por") to "Portuguese",
        listOf("ro", "ron", "rum") to "Romanian", listOf("ru", "rus") to "Russian", listOf("sk", "slk", "slo") to "Slovak",
        listOf("sl", "slv") to "Slovenian", listOf("sr", "srp") to "Serbian", listOf("sv", "swe") to "Swedish",
        listOf("ta", "tam") to "Tamil", listOf("te", "tel") to "Telugu", listOf("th", "tha") to "Thai",
        listOf("tr", "tur") to "Turkish", listOf("uk", "ukr") to "Ukrainian", listOf("vi", "vie") to "Vietnamese",
        listOf("zh", "zho", "chi") to "Chinese",
    ).flatMap { (codes, name) -> codes.map { it to name } }.toMap()

    /** The English name of a language code as files carry them ("en", "eng",
     *  "pt-BR"); codes not known are shown as they are, in capitals. */
    fun languageName(code: String): String {
        val base = code.split('-', '_').firstOrNull()?.lowercase() ?: code.lowercase()
        return languages[base] ?: code.uppercase()
    }

    // What the controls say

    private fun oneDecimal(value: Double) = BigDecimal(value).setScale(1, RoundingMode.HALF_EVEN).toPlainString()

    /** "4.2 MB/s  ·  42% loaded": how fast a stream arrives and how much of the
     *  whole has arrived. Empty when mpv does not say. */
    fun arrival(bytesASecond: Double?, loaded: Double?): String {
        val parts = mutableListOf<String>()
        // Nothing arriving (all of it is here already) is not worth saying.
        if (bytesASecond != null && bytesASecond >= 1_000) {
            parts += if (bytesASecond >= 1_000_000) "${oneDecimal(bytesASecond / 1_000_000)} MB/s"
            else "${BigDecimal(bytesASecond / 1_000).setScale(0, RoundingMode.HALF_EVEN).toPlainString()} KB/s"
        }
        if (loaded != null) parts += if (loaded >= 99.5) "100% loaded" else "${loaded.toInt()}% loaded"
        return parts.joinToString("  ·  ")
    }

    /** How much of a film has arrived, 0 to 100; null for a live stream. */
    fun shareLoaded(status: PlayerEngine.Status): Double? {
        if (status.duration <= 0) return null
        return ((status.position + (status.ahead ?: 0.0)) / status.duration * 100).coerceIn(0.0, 100.0)
    }

    /** "Ends at 21:40", by the clock, at the speed playing. */
    fun endsAt(left: Double, speed: Double, now: Instant = Instant.now(), zone: ZoneId = ZoneId.systemDefault()): String {
        val ends = now.plusMillis((left / maxOf(speed, 0.25) * 1000).toLong())
        return "Ends at ${DateTimeFormatter.ofPattern("HH:mm").withZone(zone).format(ends)}"
    }

    /** What is known of the picture and sound, a fact a line. */
    fun info(details: PlayerEngine.Details, source: String): String {
        val lines = mutableListOf<String>()
        if (details.width != null && details.height != null) {
            val rate = details.framesPerSecond?.let { ", ${formatted(Math.round(it * 1000) / 1000.0)} frames a second" } ?: ""
            lines += "Picture: ${details.width} × ${details.height}$rate"
        }
        details.video?.let { lines += "Video: $it${details.decoder?.let { d -> " (decoded by $d)" } ?: ""}" }
        details.audio?.let { lines += "Sound: $it${details.channels?.let { c -> ", $c" } ?: ""}" }
        details.container?.let { lines += "File: $it" }
        if (source.isNotEmpty()) lines += "From: $source"
        return lines.joinToString("\n")
    }

    /** A number without a needless ".0". */
    fun formatted(number: Double): String = if (number == Math.rint(number)) number.toLong().toString() else number.toString()

    /** The speed as its menu says it: "Normal", "1.5×". */
    fun speedLabel(speed: Double) = if (speed == 1.0) "Normal" else "${formatted(speed)}×"

    // Settings kept between runs

    /** What the player keeps between runs (the Rust build's key and names). */
    @Serializable
    data class Preferences(
        /** 0 silent to 1 as recorded; up to 2 (200%) when `loud` is on. */
        val volume: Double = 1.0,
        val muted: Boolean = false,
        @SerialName("subtitle_scale") val subtitleScale: Double = 1.0,
        /** Dolby and DTS sound sent on as it is, to a receiver. */
        val passthrough: Boolean = false,
        /** Settings › General: the volume may go up to 200%. */
        val loud: Boolean = false,
    ) {
        /** The highest volume allowed: 1 (100%), or 2 (200%) when `loud` is
         *  on. A TV has no volume slider, only the Sound boost in the player's
         *  menu, so there it is always 2. */
        fun top(tv: Boolean) = if (tv || loud) 2.0 else 1.0

        companion object {
            const val KEY = "player"
            /** A TV's Sound boost choices. */
            val boosts = listOf(1.0, 1.25, 1.5, 2.0)

            fun load(store: Store): Preferences = runCatching { store.get<Preferences>(KEY) }.getOrNull() ?: Preferences()
        }
    }

    // Other player apps

    /** What plays videos (the Rust build's `external_player`): 0 Haven's own
     *  player; anything else hands the video to a player app on the phone,
     *  chosen from Android's list (VLC, mpv and so on). */
    @Serializable
    data class OtherPlayer(val kind: Int = 0, val program: String = "") {
        companion object {
            const val KEY = "external_player"

            fun load(store: Store): OtherPlayer = runCatching { store.get<OtherPlayer>(KEY) }.getOrNull() ?: OtherPlayer()
        }
    }

    /** Whether a video goes to another app: one is chosen, and the video is a
     *  web address (other apps cannot open files kept inside Haven). */
    fun goesElsewhere(choice: OtherPlayer, address: String): Boolean =
        choice.kind != 0 && address.contains("://") && !address.startsWith("file://")
}
