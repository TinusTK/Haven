package net.haven.app.addons

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.put
import net.haven.app.core.Http
import net.haven.app.core.HttpAnswer
import net.haven.app.core.Store
import net.haven.app.titles.Cinemeta
import java.net.URI

/**
 * A person's Stremio account (Settings › Accounts, both Android builds):
 * signing in, the list of services the account uses (each with its address
 * and manifest), and signing out. Haven then asks those services itself, as
 * Stremio does; the account only holds the list. As the Rust build
 * (`haven-addons/src/stremio.rs`, `library/stremio.rs`): the account
 * (email and Stremio's key, never the password) is kept under
 * `stremio_account`; not in backups.
 *
 * Which add-ons follow the account: in the Google Play build nothing is
 * installed by hand, so the whole installed list is the account's (as the
 * Rust Android app did); in the GitHub build people install by hand too, so
 * only those the account brought in (kv `stremio_followed`) follow it.
 */
object Stremio {
    const val API = "https://api.strem.io/api"
    const val ACCOUNT_KEY = "stremio_account"
    const val FOLLOWED_KEY = "stremio_followed"
    /** The most an answer from Stremio may hold. */
    const val MOST_BYTES = 8 * 1024 * 1024

    /** A signed-in account, as the Rust build keeps it. Never log `authKey`. */
    @Serializable
    data class Account(val email: String, @SerialName("auth_key") val authKey: String) {
        override fun toString(): String = "Stremio account $email"
    }

    /** One service on the account: its address and its manifest (raw JSON). */
    data class Listed(val url: String, val manifest: String)

    sealed class Failure(message: String) : Exception(message) {
        /** What Stremio said, e.g. a wrong password. */
        class Refused(why: String) : Failure(why)
        class Unreachable : Failure("Stremio could not be reached")
        class BadAnswer : Failure("Stremio's answer could not be read")

        override fun equals(other: Any?): Boolean =
            other is Failure && other::class == this::class && other.message == message

        override fun hashCode(): Int = message.hashCode()
    }

    // Calls

    /** Signs in with an email address and password. */
    suspend fun login(email: String, password: String, http: Http = Http.shared): Account =
        parseLogin(call("login", buildJsonObject {
            put("type", "Login"); put("email", email.trim()); put("password", password); put("facebook", false)
        }, http))

    /** The services on the account, in the account's order. */
    suspend fun listed(account: Account, http: Http = Http.shared): List<Listed> =
        parseListed(call("addonCollectionGet", buildJsonObject {
            put("type", "AddonCollectionGet"); put("authKey", account.authKey); put("update", true)
        }, http))

    /** Ends the sign-in on Stremio's side (best effort). */
    suspend fun logout(account: Account, http: Http = Http.shared) {
        runCatching { call("logout", buildJsonObject { put("type", "Logout"); put("authKey", account.authKey) }, http) }
    }

    private suspend fun call(method: String, body: JsonObject, http: Http): JsonElement {
        val answer: HttpAnswer = try {
            http.send("$API/$method", body = body.toString().toByteArray(), limit = MOST_BYTES)
        } catch (_: Exception) {
            throw Failure.Unreachable()
        }
        return result(answer.body)
    }

    /** The `result` of an answer, or what its `error` says. */
    fun result(body: ByteArray): JsonElement {
        val answer = runCatching { Json.parseToJsonElement(body.toString(Charsets.UTF_8)).jsonObject }.getOrNull()
            ?: throw Failure.BadAnswer()
        val error = answer["error"]
        if (error != null && error !is JsonNull) {
            val message = ((error as? JsonObject)?.get("message") as? JsonPrimitive)?.takeIf { it.isString }?.content
            throw Failure.Refused(message ?: "Stremio said no")
        }
        val result = answer["result"]
        if (result == null || result is JsonNull) throw Failure.BadAnswer()
        return result
    }

    fun parseLogin(result: JsonElement): Account {
        val o = result as? JsonObject ?: throw Failure.BadAnswer()
        val key = (o["authKey"] as? JsonPrimitive)?.takeIf { it.isString }?.content?.takeIf { it.isNotEmpty() }
            ?: throw Failure.BadAnswer()
        val email = ((o["user"] as? JsonObject)?.get("email") as? JsonPrimitive)?.takeIf { it.isString }?.content ?: ""
        return Account(email, key)
    }

    fun parseListed(result: JsonElement): List<Listed> {
        val list = (result as? JsonObject)?.get("addons") as? JsonArray ?: throw Failure.BadAnswer()
        return list.mapNotNull { item ->
            val entry = item as? JsonObject ?: return@mapNotNull null
            val url = (entry["transportUrl"] as? JsonPrimitive)?.takeIf { it.isString }?.content?.trim() ?: return@mapNotNull null
            val manifest = entry["manifest"] as? JsonObject ?: return@mapNotNull null
            if (!wanted(url)) return@mapNotNull null
            Listed(url, manifest.toString())
        }
    }

    /** Leaves out Cinemeta (built into Haven) and services that only run on
     *  the computer Stremio itself runs on. */
    fun wanted(url: String): Boolean {
        val parsed = runCatching { URI(url) }.getOrNull() ?: return false
        val scheme = parsed.scheme?.lowercase() ?: return false
        if (scheme != "http" && scheme != "https") return false
        val host = parsed.host?.lowercase() ?: ""
        val local = host in setOf("127.0.0.1", "localhost", "::1", "[::1]")
        return !local && url != Cinemeta.URL
    }

    /** What to add and remove so the installed list (`have`) matches the
     *  account's (`wanted`): the Google Play build, where it is all the account's. */
    fun changes(have: List<String>, wanted: List<Listed>): Pair<List<Listed>, List<String>> =
        wanted.filter { it.url !in have } to have.filter { h -> wanted.none { it.url == h } }

    /** What to add and remove so the add-ons that came from the account
     *  (`followed`) match its list, leaving those installed by hand alone;
     *  and the new `followed` (one already installed by hand is not taken). */
    fun followedChanges(have: List<String>, followed: List<String>, wanted: List<Listed>): Triple<List<Listed>, List<String>, List<String>> {
        val add = wanted.filter { it.url !in have }
        fun listed(url: String) = wanted.any { it.url == url }
        val remove = followed.filter { !listed(it) && it in have }
        val kept = followed.filter { listed(it) } + add.map { it.url }
        return Triple(add, remove, kept)
    }

    // Keeping it

    fun account(store: Store): Account? = runCatching { store.get<Account>(ACCOUNT_KEY) }.getOrNull()

    /** Makes what is installed match the account's list. `wholeList`: the
     *  Google Play build (nothing installed by hand). True when anything changed. */
    fun follow(listed: List<Listed>, store: Store, wholeList: Boolean): Boolean {
        val have = store.addons().map { it.url }
        val (add, remove) = if (wholeList) {
            changes(have, listed)
        } else {
            val followed = runCatching { store.get<List<String>>(FOLLOWED_KEY) }.getOrNull() ?: emptyList()
            val (add, remove, kept) = followedChanges(have, followed, listed)
            if (kept.isEmpty()) store.remove(FOLLOWED_KEY) else store.put(FOLLOWED_KEY, kept)
            add to remove
        }
        if (add.isEmpty() && remove.isEmpty()) return false
        remove.forEach { runCatching { InstalledAddons.remove(it, store) } }
        add.forEach { runCatching { store.saveAddon(it.url, it.manifest) } }
        return true
    }
}
