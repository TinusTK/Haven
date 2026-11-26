package net.haven.app.core

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/** Light or dark, saved as "dark" / "light" as the Rust build did. */
@Serializable
enum class Theme {
    @SerialName("dark") Dark,
    @SerialName("light") Light,
}

/** User preferences (kv `settings`). Every field has a default, so settings
 *  saved by an older build still load. */
@Serializable
data class Settings(
    val theme: Theme = Theme.Dark,
    /** Index of the page shown when Haven was last closed. */
    @SerialName("last_page") val lastPage: Int = 0,
) {
    fun save(store: Store) = store.put(KEY, this)

    companion object {
        const val KEY = "settings"
        fun load(store: Store): Settings = store.get<Settings>(KEY) ?: Settings()
    }
}

/** The keys typed into Settings for the outside services Haven can use (kv
 *  `keys`). Empty text means "not given". Never printed. */
@Serializable
data class Keys(
    val tmdb: String = "",
    @SerialName("trakt_id") val traktId: String = "",
    @SerialName("trakt_secret") val traktSecret: String = "",
    @SerialName("simkl_id") val simklId: String = "",
    /** OMDb (omdbapi.com): Rotten Tomatoes and Metacritic scores, the age rating. */
    val omdb: String = "",
) {
    override fun toString(): String = "Keys { .. }"

    fun save(store: Store) = store.put(KEY, this)

    /** The keys to use: what was typed into Settings first, then the secrets
     *  file, then the keys built into this copy of Haven. */
    fun resolved(file: Secrets, builtIn: Secrets = Secrets.builtIn): Secrets {
        fun pick(typed: String, filed: String?, built: String?): String? {
            val t = typed.trim()
            return when {
                t.isNotEmpty() -> t
                filed != null -> filed
                !built.isNullOrEmpty() -> built
                else -> null
            }
        }
        return Secrets(
            tmdbApiKey = pick(tmdb, file.tmdbApiKey, builtIn.tmdbApiKey),
            traktClientId = pick(traktId, file.traktClientId, builtIn.traktClientId),
            traktClientSecret = pick(traktSecret, file.traktClientSecret, builtIn.traktClientSecret),
            simklClientId = pick(simklId, file.simklClientId, builtIn.simklClientId),
            omdbApiKey = pick(omdb, file.omdbApiKey, builtIn.omdbApiKey),
        )
    }

    companion object {
        const val KEY = "keys"
        fun load(store: Store): Keys = store.get<Keys>(KEY) ?: Keys()
    }
}

/** The add-ons, media servers and television sources the user switched off
 *  (kv `switched_off`). They stay installed and signed in; Haven just leaves
 *  them alone until they are switched on again. */
@Serializable
data class SwitchedOff(
    /** Add-ons, by manifest address. */
    val addons: List<String> = emptyList(),
    /** Media servers, by their number in the database. */
    val servers: List<Long> = emptyList(),
    /** Playlists and Xtream Codes accounts, by the address they are kept under. */
    @SerialName("tv_sources") val tvSources: List<String> = emptyList(),
) {
    fun save(store: Store) = store.put(KEY, this)

    companion object {
        const val KEY = "switched_off"
        fun load(store: Store): SwitchedOff = store.get<SwitchedOff>(KEY) ?: SwitchedOff()

        /** A list with `item` switched off (on it) or back on (off it). */
        fun <T> switched(list: List<T>, item: T, on: Boolean): List<T> =
            list.filter { it != item } + if (on) emptyList() else listOf(item)
    }
}
