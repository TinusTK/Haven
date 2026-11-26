package net.haven.app.core

import net.haven.app.BuildConfig
import java.io.File
import java.io.FileNotFoundException

/** API keys for the services Haven talks to. Read from `secrets.env` in
 *  Haven's data folder (`KEY=value` lines, `#` comments), or built in.
 *  Never printed: only which ones are set. */
data class Secrets(
    val tmdbApiKey: String? = null,
    val traktClientId: String? = null,
    val traktClientSecret: String? = null,
    val simklClientId: String? = null,
    val omdbApiKey: String? = null,
) {
    override fun toString(): String {
        fun set(v: String?) = if (v == null) "missing" else "set"
        return "Secrets(tmdb: ${set(tmdbApiKey)}, traktClientId: ${set(traktClientId)}, " +
            "traktClientSecret: ${set(traktClientSecret)}, simklClientId: ${set(simklClientId)}, " +
            "omdb: ${set(omdbApiKey)})"
    }

    companion object {
        /** The keys built into this copy (from the build's environment).
         *  Never a TMDB key: people add their own. */
        val builtIn: Secrets by lazy {
            fun built(v: String) = v.ifEmpty { null }
            Secrets(
                tmdbApiKey = null,
                traktClientId = built(BuildConfig.TRAKT_CLIENT_ID),
                traktClientSecret = built(BuildConfig.TRAKT_CLIENT_SECRET),
                simklClientId = built(BuildConfig.SIMKL_CLIENT_ID),
                omdbApiKey = built(BuildConfig.OMDB_API_KEY),
            )
        }

        /** A missing file is not an error: it gives empty secrets. */
        fun fromFile(file: File): Secrets = try {
            parse(file.readText())
        } catch (_: FileNotFoundException) {
            Secrets()
        } catch (e: Exception) {
            throw HavenError.Io(file.path, e.message ?: e.toString())
        }

        fun parse(text: String): Secrets {
            val map = mutableMapOf<String, String>()
            for (raw in text.split('\n')) {
                var line = raw.trim()
                if (line.isEmpty() || line.startsWith("#")) continue
                if (line.startsWith("export ")) line = line.removePrefix("export ")
                val eq = line.indexOf('=')
                if (eq < 0) continue
                map[line.substring(0, eq).trim()] = unquote(line.substring(eq + 1).trim())
            }
            fun take(key: String) = map[key]?.ifEmpty { null }
            return Secrets(
                tmdbApiKey = take("TMDB_API_KEY"),
                traktClientId = take("TRAKT_CLIENT_ID"),
                traktClientSecret = take("TRAKT_CLIENT_SECRET"),
                simklClientId = take("SIMKL_CLIENT_ID"),
                omdbApiKey = take("OMDB_API_KEY"),
            )
        }

        private fun unquote(value: String): String {
            for (quote in listOf("\"", "'")) {
                if (value.length >= 2 && value.startsWith(quote) && value.endsWith(quote)) {
                    return value.substring(1, value.length - 1)
                }
            }
            return value
        }
    }
}
