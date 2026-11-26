package net.haven.app.tracking

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import net.haven.app.core.Haven
import net.haven.app.core.Http
import net.haven.app.core.HttpAnswer
import net.haven.app.core.HttpError
import net.haven.app.core.Secrets
import net.haven.app.metadata.Tmdb
import net.haven.app.titles.ResourceUrl

/** Talks to Trakt and Simkl. */
class TrackingClient(
    val keys: Secrets,
    private val http: Http = Http.shared,
    val trakt: String = TRAKT_API,
    val simkl: String = SIMKL_API,
) {
    companion object {
        const val TRAKT_API = "https://api.trakt.tv"
        const val SIMKL_API = "https://api.simkl.com"
        const val ANSWER_LIMIT = 16 * 1024 * 1024
        /** A token is renewed when it has less than this long to live (seconds). */
        const val RENEW_BEFORE = 60 * 60L
        /** Trakt's redirect address registered for Haven's app (sent with token requests). */
        const val TRAKT_REDIRECT = "http://127.0.0.1:47219/trakt"

        /** Adds the Client Secret to a Trakt token request, when there is one. */
        fun withSecret(body: JsonObject, secret: String?): JsonObject =
            if (secret == null) body else JsonObject(body + ("client_secret" to JsonPrimitive(secret)))

        /** Percent-encodes a form or query value. */
        fun formEscape(text: String) = ResourceUrl.encoded(text)

        /** A Simkl address with what Simkl asks every request to carry: the
         *  app's client id, name and version. */
        fun simklUrl(base: String, path: String, clientId: String): String {
            val joiner = if (path.contains("?")) "&" else "?"
            return "$base$path${joiner}client_id=${formEscape(clientId)}&app-name=haven&app-version=${Haven.version}"
        }
    }

    /** Whether the keys a service needs are set. Trakt signs in with the
     *  Client ID alone (new Trakt apps get no Client Secret). */
    fun hasKeys(service: TrackingService) =
        if (service == TrackingService.Trakt) keys.traktClientId != null else keys.simklClientId != null

    private fun traktKeys(): Pair<String, String?> = (keys.traktClientId ?: throw TrackingError.NoKey("Trakt")) to keys.traktClientSecret

    private fun simklKey(): String = keys.simklClientId ?: throw TrackingError.NoKey("Simkl")

    private fun headers(account: TrackedAccount): List<Pair<String, String>> {
        val bearer = "Authorization" to "Bearer ${account.token}"
        return when (account.service) {
            TrackingService.Trakt -> listOf(bearer, "trakt-api-version" to "2", "trakt-api-key" to traktKeys().first)
            TrackingService.Simkl -> listOf(bearer, "simkl-api-key" to simklKey())
        }
    }

    private fun parse(answer: HttpAnswer): JsonElement? = runCatching { Json.parseToJsonElement(answer.text) }.getOrNull()

    /** A JSON request; an answer that is not JSON reads as null. */
    private suspend fun call(service: TrackingService, url: String, headers: List<Pair<String, String>> = emptyList(), body: JsonObject? = null): Pair<Int, JsonElement?> {
        val answer = try {
            http.send(url, headers = headers, body = body?.toString()?.toByteArray(), limit = ANSWER_LIMIT)
        } catch (e: HttpError) {
            throw TrackingError.Http(e)
        }
        if (answer.status == 401 || answer.status == 403) throw TrackingError.Refused(service.label)
        return answer.status to parse(answer)
    }

    /** A form-encoded POST to Simkl's sign-in addresses (AUTH V2). */
    private suspend fun simklForm(path: String, fields: List<Pair<String, String>>): Pair<Int, JsonElement?> {
        val body = fields.joinToString("&") { "${formEscape(it.first)}=${formEscape(it.second)}" }
        val answer = try {
            http.send(simklUrl(simkl, path, simklKey()), body = body.toByteArray(), contentType = "application/x-www-form-urlencoded", limit = ANSWER_LIMIT)
        } catch (_: HttpError.TooLarge) {
            throw TrackingError.BadAnswer()
        } catch (e: HttpError) {
            throw TrackingError.Network(e.message ?: "network error")
        }
        return answer.status to parse(answer)
    }

    private fun field(answer: JsonElement?, key: String): String =
        ((answer as? JsonObject)?.get(key) as? JsonPrimitive)?.takeIf { it.isString }?.content ?: throw TrackingError.BadAnswer()

    private fun seconds(answer: JsonElement?, key: String, fallback: Int): Int =
        Tmdb.whole((answer as? JsonObject)?.get(key))?.toInt() ?: fallback

    // Signing in

    /** Starts a sign-in and gives the code to show the user. */
    suspend fun begin(service: TrackingService): SignInCode = when (service) {
        TrackingService.Trakt -> {
            val (status, answer) = call(service, "$trakt/oauth/device/code", body = buildJsonObject { put("client_id", traktKeys().first) })
            if (status != 200) throw TrackingError.Status(status)
            SignInCode(
                service, field(answer, "user_code"), field(answer, "verification_url"), field(answer, "device_code"),
                maxOf(seconds(answer, "interval", 5), 1), seconds(answer, "expires_in", 600),
            )
        }
        TrackingService.Simkl -> {
            // The person approves the code at simkl.com/pin (the link has it filled in).
            val (status, answer) = simklForm("/oauth2/device", listOf("client_id" to simklKey(), "scope" to "media:read media:write"))
            if (status != 200) throw TrackingError.Status(status)
            val userCode = field(answer, "user_code")
            val link = runCatching { field(answer, "verification_uri_complete") }.getOrNull() ?: field(answer, "verification_uri")
            SignInCode(service, userCode, link, field(answer, "device_code"), maxOf(seconds(answer, "interval", 5), 1), seconds(answer, "expires_in", 900))
        }
    }

    /** Asks whether the user has approved the code yet; null while not. */
    suspend fun poll(code: SignInCode, now: Long): TrackedAccount? = when (code.service) {
        TrackingService.Trakt -> {
            val (id, secret) = traktKeys()
            val (status, answer) = call(
                TrackingService.Trakt, "$trakt/oauth/device/token",
                body = withSecret(buildJsonObject { put("code", code.deviceCode); put("client_id", id) }, secret),
            )
            when (status) {
                200 -> Tracking.readTraktToken(answer, now) ?: throw TrackingError.BadAnswer()
                400, 429 -> null // not approved yet, or asked too soon
                404, 409, 410, 418 -> throw TrackingError.Expired()
                else -> throw TrackingError.Status(status)
            }
        }
        TrackingService.Simkl -> {
            val (status, answer) = simklForm(
                "/oauth2/token",
                listOf("grant_type" to "urn:ietf:params:oauth:grant-type:device_code", "client_id" to simklKey(), "device_code" to code.deviceCode),
            )
            if (status == 200) {
                Tracking.readSimklToken(answer, now, null) ?: throw TrackingError.BadAnswer()
            } else {
                when (((answer as? JsonObject)?.get("error") as? JsonPrimitive)?.content) {
                    "authorization_pending", "slow_down" -> null // not approved yet (a "no" looks the same)
                    "expired_token", "invalid_grant" -> throw TrackingError.Expired()
                    else -> throw TrackingError.Status(status)
                }
            }
        }
    }

    /** A renewed account if this one's token is about to run out; null if it
     *  is still good (or never runs out). */
    suspend fun renewed(account: TrackedAccount, now: Long): TrackedAccount? {
        val expiresAt = account.expiresAt ?: return null
        val refresh = account.refresh ?: return null
        if (expiresAt - now > RENEW_BEFORE) return null
        if (account.service == TrackingService.Simkl) {
            val (status, answer) = simklForm("/oauth2/token", listOf("grant_type" to "refresh_token", "client_id" to simklKey(), "refresh_token" to refresh))
            if (status != 200) throw TrackingError.Refused("Simkl")
            // Simkl keeps the same refresh token (it may not send it again).
            return Tracking.readSimklToken(answer, now, refresh) ?: throw TrackingError.BadAnswer()
        }
        val (id, secret) = traktKeys()
        val (status, answer) = call(
            TrackingService.Trakt, "$trakt/oauth/token",
            body = withSecret(
                buildJsonObject {
                    put("refresh_token", refresh); put("client_id", id); put("redirect_uri", TRAKT_REDIRECT); put("grant_type", "refresh_token")
                },
                secret,
            ),
        )
        if (status != 200) throw TrackingError.Refused("Trakt")
        return Tracking.readTraktToken(answer, now) ?: throw TrackingError.BadAnswer()
    }

    /** Tells the service to forget the sign-in (best effort). */
    suspend fun revoke(account: TrackedAccount) {
        when (account.service) {
            TrackingService.Trakt -> {
                val (id, secret) = traktKeys()
                call(TrackingService.Trakt, "$trakt/oauth/revoke", body = withSecret(buildJsonObject { put("token", account.token); put("client_id", id) }, secret))
            }
            TrackingService.Simkl -> simklForm("/oauth2/revoke", listOf("client_id" to simklKey(), "token" to account.token))
        }
    }

    // Keeping in step

    private suspend fun post(account: TrackedAccount, path: String, body: JsonObject) {
        val url = if (account.service == TrackingService.Trakt) "$trakt$path" else simklUrl(simkl, path, simklKey())
        val (status, _) = call(account.service, url, headers(account), body)
        if (status !in 200..299) throw TrackingError.Status(status)
    }

    /** Marks a film or an episode as watched, now. */
    suspend fun markWatched(account: TrackedAccount, subject: Subject) = post(account, "/sync/history", Tracking.body(subject))

    /** Takes a film or episode off the account's watched history. */
    suspend fun markUnwatched(account: TrackedAccount, subject: Subject) = post(account, "/sync/history/remove", Tracking.body(subject))

    /** Puts a film or series on the service's Watchlist, or takes it off. Simkl
     *  has no way to take a title off its list without also erasing its
     *  watched history, so there a removal is not passed on. */
    suspend fun setWatchlist(account: TrackedAccount, subject: Subject, on: Boolean) {
        val whole = subject.copy(episode = null)
        when {
            account.service == TrackingService.Trakt && on -> post(account, "/sync/watchlist", Tracking.body(whole))
            account.service == TrackingService.Trakt -> post(account, "/sync/watchlist/remove", Tracking.body(whole))
            on -> post(account, "/sync/add-to-list", Tracking.body(whole, simklList = "plantowatch"))
            else -> Unit
        }
    }

    /** What the account has watched: films, and each episode of a series. */
    suspend fun watched(account: TrackedAccount): List<Watched> {
        val headers = headers(account)
        val found = mutableListOf<Watched>()
        for (series in listOf(false, true)) {
            val url = when {
                account.service == TrackingService.Trakt -> "$trakt/sync/watched/${if (series) "shows" else "movies"}"
                series -> simklUrl(simkl, "/sync/all-items/shows?extended=full&episode_watched_at=yes", simklKey())
                else -> simklUrl(simkl, "/sync/all-items/movies/completed", simklKey())
            }
            val (status, answer) = call(account.service, url, headers)
            if (status !in 200..299) throw TrackingError.Status(status)
            found += Tracking.readWatched(answer, series)
        }
        return found
    }

    /** The films and series on the service's Watchlist. */
    suspend fun watchlist(account: TrackedAccount): List<Listed> {
        val headers = headers(account)
        val listed = mutableListOf<Listed>()
        for (series in listOf(false, true)) {
            val what = if (series) "shows" else "movies"
            val url = if (account.service == TrackingService.Trakt) "$trakt/sync/watchlist/$what"
            else simklUrl(simkl, "/sync/all-items/$what/plantowatch", simklKey())
            val (status, answer) = call(account.service, url, headers)
            if (status !in 200..299) throw TrackingError.Status(status)
            listed += Tracking.readListed(answer, series)
        }
        return listed
    }
}
