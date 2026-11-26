package net.haven.app.metadata

import net.haven.app.core.Store
import net.haven.app.titles.MetaDetail
import net.haven.app.titles.ResourceUrl
import java.util.Locale

/** The services that carry a title (subscription, rental, purchase), as TMDB
 *  passes them on from JustWatch, and the country they are shown for. */
object WhereToWatch {
    /** Where the chosen country is kept (kv), as a two-letter code; missing
     *  means Automatic (the device's own country). */
    const val COUNTRY_KEY = "country"
    /** Looked in, in this order, when the chosen country has nothing. */
    val fallbacks = listOf("US", "GB", "CA", "AU")

    /** The countries offered in Settings: two-letter code and name. */
    val countries: List<Pair<String, String>> = listOf(
        "AR" to "Argentina", "AU" to "Australia", "AT" to "Austria", "BE" to "Belgium",
        "BR" to "Brazil", "CA" to "Canada", "CL" to "Chile", "CO" to "Colombia",
        "CZ" to "Czechia", "DK" to "Denmark", "FI" to "Finland", "FR" to "France",
        "DE" to "Germany", "GR" to "Greece", "HK" to "Hong Kong", "HU" to "Hungary",
        "IN" to "India", "ID" to "Indonesia", "IE" to "Ireland", "IL" to "Israel",
        "IT" to "Italy", "JP" to "Japan", "MY" to "Malaysia", "MX" to "Mexico",
        "NL" to "Netherlands", "NZ" to "New Zealand", "NO" to "Norway", "PH" to "Philippines",
        "PL" to "Poland", "PT" to "Portugal", "SG" to "Singapore", "ZA" to "South Africa",
        "KR" to "South Korea", "ES" to "Spain", "SE" to "Sweden", "CH" to "Switzerland",
        "TH" to "Thailand", "TR" to "Türkiye", "GB" to "United Kingdom", "US" to "United States",
    )

    const val NO_KEY_NOTE = "Add a TMDB key in Settings to see which streaming services, rental and download shops have this."
    const val LOOKING_NOTE = "Looking for services that have this…"
    const val CREDIT_NOTE = "Tap a service to see it on JustWatch. Availability by JustWatch."

    /** Haven's own tile for a service: the letters on it, its background and
     *  its ink (0xRRGGBB). Our own designs (no company's logo), matched by name. */
    data class TileLook(val key: String, val letters: String, val background: Int, val ink: Int)

    val tileLooks: List<TileLook> = listOf(
        TileLook("netflix", "N", 0x1a0709, 0xe5484d),
        TileLook("amazon prime", "pv", 0x0b2236, 0x5ab0f0),
        TileLook("prime video", "pv", 0x0b2236, 0x5ab0f0),
        TileLook("amazon video", "a", 0x0b2236, 0x5ab0f0),
        TileLook("amazon", "a", 0x0b2236, 0x5ab0f0),
        TileLook("disney", "D+", 0x101a40, 0x8fb0ff),
        TileLook("hbo max", "max", 0x1c1030, 0xb993ff),
        TileLook("max", "max", 0x1c1030, 0xb993ff),
        TileLook("apple tv", "tv", 0x1d1f24, 0xe9ebef),
        TileLook("apple itunes", "tv", 0x1d1f24, 0xe9ebef),
        TileLook("google play", "G", 0x0d2418, 0x5fd18f),
        TileLook("youtube", "YT", 0x240808, 0xff5a5a),
        TileLook("stan", "S", 0x0a1a33, 0x4a9cff),
        TileLook("binge", "B", 0x21082a, 0xe86bf5),
        TileLook("foxtel", "F", 0x2a1206, 0xff8a3d),
        TileLook("kayo", "K", 0x062a20, 0x45e0b0),
        TileLook("paramount", "P+", 0x081a3a, 0x5c8dff),
        TileLook("hulu", "h", 0x062612, 0x3ee07a),
        TileLook("peacock", "pk", 0x141414, 0xf3c94b),
        TileLook("tubi", "tb", 0x2a1a05, 0xffb347),
        TileLook("pluto", "pl", 0x1f1a05, 0xf5e04a),
        TileLook("sbs", "SBS", 0x1e1a05, 0xf7d24a),
        TileLook("abc iview", "iv", 0x062224, 0x46d6dc),
        TileLook("7plus", "7+", 0x260808, 0xff6b6b),
        TileLook("9now", "9N", 0x07173a, 0x6aa2ff),
        TileLook("10 play", "10", 0x1a1a1a, 0xf0f0f0),
        TileLook("bbc iplayer", "iP", 0x1f0a26, 0xe07bff),
        TileLook("itvx", "X", 0x08232a, 0x4fd2e8),
        TileLook("channel 4", "4", 0x14141f, 0xb8b8ff),
        TileLook("now tv", "NOW", 0x0b2a1f, 0x3fe0a0),
        TileLook("sky", "sky", 0x0a1630, 0x6a9cff),
        TileLook("crunchyroll", "CR", 0x2a1405, 0xff9a3d),
        TileLook("mubi", "M", 0x161616, 0xeeeeee),
        TileLook("microsoft", "MS", 0x0f1a24, 0x7cc4ff),
    )

    /** One tile on a title's page. */
    data class Tile(
        val name: String,
        /** "Subscription, Rent"… */
        val ways: String,
        /** The group's name on the first tile of each group; empty after. */
        val group: String,
        val letters: String,
        /** Background and ink as 0xRRGGBB. */
        val tint: Int,
        val ink: Int,
    )

    /** The tile for a service name: letters, background, ink. */
    fun tile(name: String): Triple<String, Int, Int> {
        val lower = name.lowercase()
        tileLooks.firstOrNull { lower.contains(it.key) }?.let { return Triple(it.letters, it.background, it.ink) }
        // Any other service: its first letter in Haven's gold.
        val first = name.firstOrNull { it.isLetterOrDigit() }?.uppercase() ?: ""
        return Triple(first, 0x1b1d24, 0xf0c36a)
    }

    /** The four groups shown, and the ways of watching each one holds. */
    val groups: List<Pair<String, List<String>>> = listOf(
        "Stream" to listOf("Subscription"),
        "Free" to listOf("Free", "Free with ads"),
        "Rent" to listOf("Rent"),
        "Buy" to listOf("Buy"),
    )

    /** JustWatch's search for a title in a country (JustWatch writes the United Kingdom as "uk"). */
    fun justWatchLink(country: String, title: String): String {
        val code = if (country.uppercase() == "GB") "uk" else country.lowercase()
        return "https://www.justwatch.com/$code/search?q=${ResourceUrl.encoded(title)}"
    }

    /** A service's name as it fits under its tile: without the company in
     *  front ("Amazon Prime Video" is "Prime Video") or " with Ads". */
    fun shortName(name: String): String {
        var short = name.trim()
        for (suffix in listOf(" with Ads", " with ads")) {
            if (short.endsWith(suffix)) {
                short = short.removeSuffix(suffix)
                break
            }
        }
        if (short.startsWith("Amazon ") && short.removePrefix("Amazon ").startsWith("Prime")) short = short.removePrefix("Amazon ")
        return short
    }

    /** One tile per service per group, groups in order; the first tile of
     *  each group carries the group's name. Two listings of one service
     *  ("Amazon Prime Video" and "… with Ads") show once. */
    fun tiles(services: List<Service>): List<Tile> {
        val rows = mutableListOf<Tile>()
        for ((group, ways) in groups) {
            var first = true
            val shown = mutableListOf<Pair<String, Int>>()
            for (service in services.filter { s -> s.ways.any { it in ways } }) {
                val (letters, tint, ink) = tile(service.name)
                if (letters to tint in shown) continue
                shown += letters to tint
                rows += Tile(shortName(service.name), service.ways.joinToString(", "), if (first) group else "", letters, tint, ink)
                first = false
            }
        }
        return rows
    }

    fun countryName(code: String): String = countries.firstOrNull { it.first == code }?.second ?: code

    /** The region part of a locale such as "en-AU", "en_AU.UTF-8" or
     *  "zh-Hant-HK", if it is one of the countries offered. */
    fun region(locale: String): String? {
        val head = locale.split('.', '@').firstOrNull() ?: return null
        return head.split('_', '-').drop(1).map { it.uppercase() }
            .firstOrNull { part -> part.length == 2 && countries.any { it.first == part } }
    }

    /** The country this device is set to; the United States if it does not say. */
    fun deviceCountry(): String {
        val region = Locale.getDefault().country.uppercase()
        if (countries.any { it.first == region }) return region
        return region(Locale.getDefault().toLanguageTag()) ?: "US"
    }

    /** The country the user picked, if they did. */
    fun chosenCountry(store: Store): String? =
        runCatching { store.get<String>(COUNTRY_KEY) }.getOrNull()?.takeIf { code -> countries.any { it.first == code } }

    /** The country in use: the user's pick, else the device's own (looked at
     *  each time, so it follows the device until one is picked). */
    fun country(store: Store): String = chosenCountry(store) ?: deviceCountry()

    /** The list in Settings: "Automatic (the device's country)" first, then every country. */
    fun settingsList(): List<String> = listOf("Automatic (${countryName(deviceCountry())})") + countries.map { it.second }

    /** Which line of [settingsList] is chosen. */
    fun settingsIndex(store: Store): Int {
        val code = chosenCountry(store) ?: return 0
        return countries.indexOfFirst { it.first == code } + 1
    }

    /** Line 0 is Automatic: forget the pick and follow the device. */
    fun choose(index: Int, store: Store) {
        if (index <= 0) store.remove(COUNTRY_KEY) else if (index - 1 < countries.size) store.put(COUNTRY_KEY, countries[index - 1].first)
    }

    /** What the title page shows under "Where to watch". */
    data class Found(
        val tiles: List<Tile>,
        /** JustWatch's search for the title; empty when there is nothing. */
        val link: String,
        /** The country shown ("United Kingdom (nothing in Australia)"), or why there is nothing. */
        val note: String,
    )

    /** Looks up which services carry a title, in `chosen` first and then the fallback countries. */
    suspend fun lookUp(meta: MetaDetail, tmdb: Tmdb, chosen: String): Found {
        val series = meta.kind == "series"
        val known = meta.tmdbId?.toLongOrNull() ?: Tmdb.splitId(meta.id)?.second
        val imdb = listOfNotNull(meta.imdbId, meta.id).firstOrNull()?.takeIf { Tmdb.isImdbId(it) }
        val wanted = listOf(chosen) + fallbacks.filter { it != chosen }
        return try {
            var number = known
            if (number == null && imdb != null) number = tmdb.numberOf(series, imdb)
            val offers = number?.let { tmdb.offers(series, it, wanted) }
            if (offers != null) {
                val note = if (offers.country == chosen) countryName(offers.country)
                else "${countryName(offers.country)} (nothing in ${countryName(chosen)})"
                Found(tiles(offers.services), justWatchLink(offers.country, meta.name), note)
            } else {
                Found(emptyList(), "", "No service is known to have this in ${countryName(chosen)} or the other countries Haven looks in.")
            }
        } catch (e: Exception) {
            Found(emptyList(), "", "Could not look this up: ${e.message}.")
        }
    }
}
