package net.haven.app.addons

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonPrimitive
import net.haven.app.core.Http
import net.haven.app.core.HttpError
import net.haven.app.core.Log
import net.haven.app.core.Store
import net.haven.app.core.SwitchedOff
import net.haven.app.titles.Catalog
import net.haven.app.titles.Catalogues
import net.haven.app.titles.Cinemeta
import net.haven.app.titles.JObj
import net.haven.app.titles.MediaStream
import net.haven.app.titles.ResourceUrl
import net.haven.app.titles.Serves
import net.haven.app.titles.Source
import net.haven.app.titles.TitlesError

// Add-ons: web services that give Haven catalogues, details and streams, in
// the public Stremio add-on protocol. An add-on is an address that serves a
// `manifest.json` describing what it offers; everything else is a JSON file
// next to it (`catalog/<type>/<id>.json`, `meta/…`, `stream/…`).
//
// In both Android builds: the GitHub one installs them by address too (the
// Add-ons page, BuildConfig.ADDON_SCREENS); the Google Play one only gets
// them through the Stremio sign-in.

sealed class AddonError(message: String) : Exception(message) {
    class InvalidAddress(val address: String) : AddonError("\"$address\" is not an add-on address")
    class Http(val error: HttpError) : AddonError(error.message ?: "network error")
    class BadManifest(val why: String) : AddonError("the add-on's manifest could not be read: $why")
    class IncompleteManifest(val field: String) : AddonError("the add-on's manifest has no $field")
    class BadResponse(val why: String) : AddonError("the add-on's answer could not be read: $why")
    class NotText : AddonError("the add-on's answer was not text")
    class UnknownTitle : AddonError("the add-on does not know this title")

    override fun equals(other: Any?): Boolean =
        other is AddonError && other::class == this::class && other.message == message

    override fun hashCode(): Int = message.hashCode()

    companion object {
        fun of(error: TitlesError): AddonError = when (error) {
            is TitlesError.InvalidAddress -> InvalidAddress(error.address)
            is TitlesError.Http -> Http(error.error)
            is TitlesError.NotText -> NotText()
            is TitlesError.BadResponse -> BadResponse(error.why)
            is TitlesError.UnknownTitle -> UnknownTitle()
        }
    }
}

/** One kind of answer an add-on gives. The short form (just a name) uses the
 *  manifest's own types and id prefixes. */
data class AddonResource(val name: String, val types: List<String>? = null, val idPrefixes: List<String>? = null) {
    companion object {
        fun from(value: JsonElement): AddonResource {
            if (value is JsonPrimitive && value.isString) return AddonResource(value.content)
            val o = JObj.of(value) ?: throw JObj.Wrong("resources")
            return AddonResource(o.string("name"), o.optionalStrings("types"), o.optionalStrings("idPrefixes"))
        }
    }
}

/** What an add-on says about itself. Fields Haven does not use are ignored,
 *  so the raw JSON is what gets stored. */
data class AddonManifest(
    val id: String,
    val name: String,
    val version: String = "",
    val description: String? = null,
    val logo: String? = null,
    val types: List<String> = emptyList(),
    val catalogs: List<Catalog> = emptyList(),
    /** What the add-on can be asked for: `catalog`, `meta`, `stream`… */
    val resources: List<AddonResource> = emptyList(),
    /** If set, the add-on only knows titles whose id starts with one of these. */
    val idPrefixes: List<String>? = null,
) {
    /** Whether this add-on can be asked for `resource` about the title `id` of type `kind`. */
    fun provides(resource: String, kind: String, id: String): Boolean {
        val found = resources.firstOrNull { it.name == resource } ?: return false
        if (kind !in (found.types ?: types)) return false
        val prefixes = found.idPrefixes ?: idPrefixes ?: return true
        return prefixes.any { id.startsWith(it) }
    }

    /** This add-on as a source of catalogues, details and streams. */
    fun source(url: String) = Source(
        url, name, catalogs, resources.map { Serves(it.name, it.types ?: types, it.idPrefixes ?: idPrefixes) },
    )

    companion object {
        /** Reads a manifest; refuses one without an id or a name. */
        fun parse(json: String): AddonManifest {
            val manifest = try {
                from(JObj.parse(json))
            } catch (wrong: JObj.Wrong) {
                throw AddonError.BadManifest(
                    if (wrong.field == "(top)") "not an object" else "${wrong.field} is missing or has the wrong type",
                )
            } catch (_: Exception) {
                throw AddonError.BadManifest("not JSON")
            }
            if (manifest.id.isBlank()) throw AddonError.IncompleteManifest("id")
            if (manifest.name.isBlank()) throw AddonError.IncompleteManifest("name")
            return manifest
        }

        private fun from(o: JObj): AddonManifest {
            val catalogs = when (val list = o["catalogs"]) {
                null -> emptyList()
                is JsonArray -> list.map { Catalog.from(JObj.of(it) ?: throw JObj.Wrong("catalogs")) }
                else -> throw JObj.Wrong("catalogs")
            }
            val resources = when (val list = o["resources"]) {
                null -> emptyList()
                is JsonArray -> list.map { AddonResource.from(it) }
                else -> throw JObj.Wrong("resources")
            }
            return AddonManifest(
                id = o.string("id"), name = o.string("name"), version = o.optionalString("version") ?: "",
                description = o.optionalString("description"), logo = o.optionalString("logo"),
                types = o.strings("types"), catalogs = catalogs, resources = resources,
                idPrefixes = o.optionalStrings("idPrefixes"),
            )
        }

        /** Turns what the user typed or pasted into the add-on's manifest
         *  address: a full `…/manifest.json` address, the add-on's base
         *  address, or a `stremio://` install link. */
        fun manifestUrl(input: String): String {
            val trimmed = input.trim()
            val invalid = AddonError.InvalidAddress(trimmed)
            if (trimmed.isEmpty()) throw invalid
            val address = if (trimmed.startsWith("stremio://")) "https://" + trimmed.removePrefix("stremio://") else trimmed
            val url = runCatching { Http.webUrl(address) }.getOrNull() ?: throw invalid
            if (url.host.isEmpty()) throw invalid
            var path = url.encodedPath
            if (!path.endsWith("/manifest.json")) path = path.trimEnd('/') + "/manifest.json"
            return url.newBuilder().fragment(null).encodedPath(path).build().toString()
        }
    }
}

/** Talks to add-ons over the web. Their catalogues and details are read
 *  with [Catalogues], like the built-in catalogue's. */
class AddonClient(val catalogues: Catalogues = Catalogues()) {
    companion object {
        const val MANIFEST_LIMIT = 1024 * 1024
        const val ANSWER_LIMIT = 8 * 1024 * 1024
    }

    private suspend fun text(url: String, limit: Int): String = try {
        catalogues.text(url, limit)
    } catch (e: TitlesError) {
        throw AddonError.of(e)
    }

    /** The add-on's manifest as the raw JSON it served. */
    suspend fun manifestJson(manifestUrl: String): String = text(manifestUrl, MANIFEST_LIMIT)

    /** The add-on's streams for a film, or for one episode (`id` is then the episode's id). */
    suspend fun streams(manifestUrl: String, kind: String, id: String): List<MediaStream> = try {
        MediaStream.parseList(text(ResourceUrl.make(manifestUrl, "stream", kind, id), ANSWER_LIMIT))
    } catch (e: TitlesError) {
        throw AddonError.of(e)
    }
}

/** The installed add-ons: reading them from the database, installing,
 *  switching on and off, removing. */
data class InstalledAddons(
    val rows: List<Row> = emptyList(),
    /** Sources to ask: the switched-on add-ons, in install order. */
    val sources: List<Source> = emptyList(),
) {
    /** One installed add-on as the Add-ons page lists it. */
    data class Row(
        val url: String,
        val name: String,
        val description: String,
        val version: String,
        /** Catalogues that can be shown as shelves. */
        val catalogs: Int,
        val enabled: Boolean,
    )

    /** What installing an address came to, in words for the Add-ons page. */
    sealed class Outcome {
        data class Installed(val name: String) : Outcome()
        /** Cinemeta is built in. */
        data object BuiltIn : Outcome()
        data class Failed(val why: String) : Outcome()

        val message: String
            get() = when (this) {
                is Installed -> "Installed $name."
                BuiltIn -> "Cinemeta is built into Haven: there is no need to install it."
                is Failed -> why
            }

        val isError: Boolean get() = this is Failed
    }

    companion object {
        /** Reads the installed add-ons. The Cinemeta copy older versions
         *  installed as an add-on is removed (it is built in now); a manifest
         *  that no longer reads is skipped; a switched-off add-on is listed
         *  but never asked. */
        fun load(store: Store): InstalledAddons {
            val stored = runCatching { store.addons() }.getOrElse {
                Log.note("could not read the installed add-ons: ${it.message}")
                emptyList()
            }
            val off = runCatching { SwitchedOff.load(store).addons }.getOrDefault(emptyList())
            val rows = mutableListOf<Row>()
            val sources = mutableListOf<Source>()
            for (addon in stored) {
                if (addon.url == Cinemeta.URL) {
                    runCatching { store.removeAddon(addon.url) }
                    continue
                }
                val manifest = try {
                    AddonManifest.parse(addon.manifest)
                } catch (e: Exception) {
                    Log.note("skipping add-on ${Log.plainAddress(addon.url)}: ${e.message}")
                    continue
                }
                val enabled = addon.url !in off
                rows += Row(
                    addon.url, manifest.name, manifest.description ?: "", manifest.version,
                    manifest.catalogs.count { !it.needsInput }, enabled,
                )
                if (enabled) sources += manifest.source(addon.url)
            }
            return InstalledAddons(rows, sources)
        }

        /** Fetches the manifest at the address the user gave and installs it. */
        suspend fun install(address: String, store: Store, client: AddonClient = AddonClient()): Outcome {
            val url = try {
                AddonManifest.manifestUrl(address)
            } catch (e: AddonError) {
                return Outcome.Failed("Could not install the add-on: ${e.message}.")
            }
            if (url == Cinemeta.URL) return Outcome.BuiltIn
            val (json, manifest) = try {
                val json = client.manifestJson(url)
                json to AddonManifest.parse(json)
            } catch (e: Exception) {
                return Outcome.Failed("Could not install the add-on: ${e.message}.")
            }
            return try {
                store.saveAddon(url, json)
                Outcome.Installed(manifest.name)
            } catch (e: Exception) {
                Outcome.Failed("Could not save the add-on: ${e.message}")
            }
        }

        /** Switches an add-on on or off; it stays installed. */
        fun setEnabled(url: String, on: Boolean, store: Store) {
            val off = SwitchedOff.load(store)
            off.copy(addons = SwitchedOff.switched(off.addons, url, on)).save(store)
        }

        /** Removes an add-on, and forgets that it was switched off. */
        fun remove(url: String, store: Store) {
            store.removeAddon(url)
            setEnabled(url, on = true, store = store)
        }
    }
}
