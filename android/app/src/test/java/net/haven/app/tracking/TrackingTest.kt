package net.haven.app.tracking

import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import net.haven.app.core.Change
import net.haven.app.core.Secrets
import net.haven.app.core.Store
import net.haven.app.testing.FakeWeb
import net.haven.app.titles.MetaDetail
import net.haven.app.titles.Video
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

private fun json(text: String): JsonElement = Json.parseToJsonElement(text)

/** The Rust build's tracking tests (haven-tracking lib and sync), ported as
 *  the Swift build has them. The PKCE test is left out: PKCE is the
 *  desktop's browser sign-in; phones link with a code. */
class TrackingRulesTest {
    @Test
    fun simklAddressesCarryTheAppsNameAndVersion() {
        val url = TrackingClient.simklUrl(TrackingClient.SIMKL_API, "/sync/history", "abc")
        assertTrue(url, url.startsWith("https://api.simkl.com/sync/history?client_id=abc&app-name=haven&app-version="))
        assertTrue(TrackingClient.simklUrl(TrackingClient.SIMKL_API, "/x?y=1", "abc").contains("/x?y=1&client_id=abc"))
        assertEquals("media%3Aread%20media%3Awrite", TrackingClient.formEscape("media:read media:write"))
    }

    @Test
    fun aSimklTokenKeepsItsRefreshToken() {
        val account = Tracking.readSimklToken(json("""{"access_token": "a", "refresh_token": "r", "expires_in": 604800}"""), 100, null)!!
        assertTrue(account.refresh == "r" && account.expiresAt == 604_900L)
        assertEquals("r", Tracking.readSimklToken(json("""{"access_token": "b", "expires_in": 10}"""), 0, "r")!!.refresh)
    }

    @Test
    fun aSecretGoesAlongOnlyWhenThereIsOne() {
        val body = buildJsonObject { put("a", 1) }
        assertEquals(json("""{"a": 1}"""), TrackingClient.withSecret(body, null))
        assertEquals(json("""{"a": 1, "client_secret": "s"}"""), TrackingClient.withSecret(body, "s"))
    }

    @Test
    fun traktsWatchedListIsRead() {
        val shows = json(
            """[{"last_watched_at": "2026-01-02T03:04:05.000Z", "show": {"title": "A Show", "ids": {"imdb": "tt1"}},
              "seasons": [{"number": 2, "episodes": [{"number": 5, "last_watched_at": "2026-01-02T03:04:05.000Z"}]}]},
             {"show": {"title": "No id", "ids": {}}, "seasons": []}]""",
        )
        val found = Tracking.readWatched(shows, series = true)
        assertEquals(1, found.size)
        assertEquals(2 to 5, found[0].episode)
        assertEquals(1_767_323_045L, found[0].at)
        val films = json("""[{"movie": {"title": "F", "ids": {"imdb": "tt2"}}, "last_watched_at": "2026-01-02T03:04:05Z"}]""")
        assertNull(Tracking.readWatched(films, series = false)[0].episode)
        assertEquals(1_767_323_045L, Tracking.readWatched(films, series = false)[0].at)
    }

    @Test
    fun simklsWatchedAnswerIsReadLikeTrakts() {
        val shows = json(
            """{"shows": [{"status": "watching", "show": {"title": "A Show", "ids": {"simkl": 1, "imdb": "tt9"}},
               "seasons": [{"number": 1, "episodes": [{"number": 2, "watched_at": "2026-11-20T10:00:00Z"}]}]}]}""",
        )
        val found = Tracking.readWatched(shows, series = true)
        assertTrue(found.size == 1 && found[0].imdb == "tt9" && found[0].episode?.second == 2 && found[0].at > 0)
        val films = json("""{"movies": [{"last_watched_at": "2026-11-20T10:00:00Z", "movie": {"title": "A Film", "ids": {"imdb": "tt5"}}}]}""")
        assertEquals("tt5", Tracking.readWatched(films, series = false).first().imdb)
    }

    @Test
    fun subjectsNeedAnImdbId() {
        assertEquals(Subject("tt0111161", false), Tracking.subject("movie", "tt0111161", "tt0111161"))
        assertEquals(Subject("tt0903747", true, 2 to 5), Tracking.subject("series", "tt0903747", "tt0903747:2:5"))
        // The series itself, or an episode id in another shape.
        assertNull(Tracking.subject("series", "tt0903747", null)?.episode)
        assertNull(Tracking.subject("series", "tt0903747", "tt1:2:5")?.episode)
        assertNull(Tracking.subject("series", "tt0903747", "tt0903747:2:5:9")?.episode)
        assertNull(Tracking.subject("movie", "srv:1:abc", null))
        assertNull(Tracking.subject("movie", "hv1", null))
    }

    @Test
    fun bodiesNameFilmsSeriesAndEpisodes() {
        val film = Subject("tt1", false)
        assertEquals(json("""{"movies": [{"ids": {"imdb": "tt1"}}]}"""), Tracking.body(film))
        assertEquals(
            json("""{"shows": [{"ids": {"imdb": "tt2"}, "seasons": [{"number": 1, "episodes": [{"number": 3}]}]}]}"""),
            Tracking.body(Subject("tt2", true, 1 to 3)),
        )
        assertEquals(json("""{"movies": [{"ids": {"imdb": "tt1"}, "to": "plantowatch"}]}"""), Tracking.body(film, simklList = "plantowatch"))
    }

    @Test
    fun watchlistsOfBothServicesAreRead() {
        val trakt = json("""[{"movie": {"title": "A Film", "year": 2020, "ids": {"imdb": "tt10", "trakt": 1}}}, {"movie": {"title": "No IMDb id", "year": 2021, "ids": {"trakt": 2}}}]""")
        assertEquals(listOf(Listed("tt10", false, "A Film", "2020")), Tracking.readListed(trakt, series = false))
        val shows = Tracking.readListed(json("""{"shows": [{"show": {"title": "A Show", "ids": {"imdb": "tt20"}}}]}"""), series = true)
        assertTrue(shows[0].title == "A Show" && shows[0].year == "" && shows[0].series)
        assertTrue(Tracking.readListed(JsonNull, series = true).isEmpty())
    }

    @Test
    fun aTraktTokenKnowsWhenItRunsOut() {
        val account = Tracking.readTraktToken(json("""{"access_token": "a", "refresh_token": "r", "expires_in": 86400, "created_at": 1000}"""), 5)!!
        assertEquals(87_400L, account.expiresAt)
        assertEquals("r", account.refresh)
        assertFalse(account.toString().contains("token"))
        assertNull(Tracking.readTraktToken(json("{}"), 5))
    }

    @Test
    fun withoutKeysNothingIsAsked() = runBlocking {
        val client = TrackingClient(Secrets())
        assertTrue(!client.hasKeys(TrackingService.Trakt) && !client.hasKeys(TrackingService.Simkl))
        assertEquals(TrackingError.NoKey("Trakt"), runCatching { client.begin(TrackingService.Trakt) }.exceptionOrNull())
    }

    @Test
    fun whatAccountsHoldIsAddedOnce() {
        val store = Store.inMemory()
        val listed = listOf(Listed("tt1", true, "A Show", "2020"))
        assertEquals(1, Sync.addListed(store, listed, 100))
        assertEquals(0, Sync.addListed(store, listed, 100))
        val watched = listOf(Watched("tt1", "A Show", 1 to 2, 5))
        assertEquals(1, Sync.addWatched(store, watched))
        assertEquals(0, Sync.addWatched(store, watched))
        val saved = store.progress("series", "tt1", "tt1:1:2")!!
        assertTrue(saved.finished && saved.detail == "S1 E2" && saved.updatedAt == 5000L)
    }

    @Test
    fun accountsAndOptionsAreKeptInTheRustBuildsShape() {
        val store = Store.inMemory()
        // As the Rust build saved them.
        store.putRaw(TrackedAccount.KEY, """[{"service": "trakt", "token": "t", "refresh": "r", "expires_at": 99}, {"service": "simkl", "token": "s", "refresh": null, "expires_at": null}]""")
        val rust = TrackedAccount.load(store)
        assertEquals(listOf(TrackedAccount(TrackingService.Trakt, "t", "r", 99), TrackedAccount(TrackingService.Simkl, "s")), rust)
        val saved = Store.json.encodeToString(TrackedAccount.serializer(), TrackedAccount(TrackingService.Simkl, "x", expiresAt = 5))
        assertTrue(saved, saved.contains("\"service\":\"simkl\"") && saved.contains("\"expires_at\":5"))
        store.putRaw(Sync.Options.KEY, """{"watched": false, "last_sync": 7, "off": ["Simkl"]}""")
        val options = Sync.Options.load(store)
        assertTrue("the old switches are gone: always on", options.watched && options.lastSync == 7L && options.off == listOf("Simkl"))
        val text = Store.json.encodeToString(Sync.Options.serializer(), options)
        assertTrue(text, text.contains("\"last_sync\":7") && text.contains("\"last_pull\":0"))
    }

    @Test
    fun onlyWhatChangedOnTheAccountCounts() {
        fun film(imdb: String) = Listed(imdb, false, imdb, "")
        fun episode(number: Int) = Watched("tt9", "S", 1 to number, 0)
        // The first pull: everything comes in, nothing is taken off.
        val first = Sync.changesSince(null, Sync.Pulled(TrackingService.Trakt, listOf(film("tt1")), listOf(episode(1))))
        assertTrue(first.addedListed.size == 1 && first.addedWatched.size == 1)
        assertTrue(first.goneListed.isEmpty() && first.goneWatched.isEmpty())
        val before = Sync.Seen(setOf("movie|tt1", "movie|tt2"), setOf("series|tt9|tt9:1:1", "series|tt9|tt9:1:2"))
        val changes = Sync.changesSince(before, Sync.Pulled(TrackingService.Trakt, listOf(film("tt1"), film("tt3")), listOf(episode(1))))
        assertEquals(listOf("tt3"), changes.addedListed.map { it.imdb })
        assertEquals(listOf("movie|tt2"), changes.goneListed)
        assertTrue(changes.addedWatched.isEmpty())
        assertEquals(listOf("series|tt9|tt9:1:2"), changes.goneWatched)
        // A part that could not be read changes nothing.
        val unread = Sync.changesSince(before, Sync.Pulled(TrackingService.Simkl))
        assertTrue(unread.goneListed.isEmpty() && unread.goneWatched.isEmpty() && unread.seen == before)
        // A change still waiting here wins.
        val waiting = listOf(Change(what = "watched", kind = "series", titleId = "tt9", videoId = "tt9:1:2", createdAt = 0))
        assertTrue(Sync.waiting(waiting, "series|tt9|tt9:1:2") && !Sync.waiting(waiting, "series|tt9|tt9:1:3"))
        assertEquals(Triple("series", "tt9", "tt9:1:2"), Sync.keyParts("series|tt9|tt9:1:2"))
    }

    @Test
    fun theSeenListsAreKeptAsTheOtherAppsKeepThem() {
        val store = Store.inMemory()
        // As the Rust and Swift builds write kv `sync_seen`.
        store.putRaw(Sync.Seen.KEY, """{"Trakt":{"watchlist":["movie|tt1"],"watched":["movie|tt6|tt6"]}}""")
        val seen = store.get<Map<String, Sync.Seen>>(Sync.Seen.KEY)!!
        assertEquals(Sync.Seen(setOf("movie|tt1"), setOf("movie|tt6|tt6")), seen["Trakt"])
    }
}

/** Talking to the services, a whole sync round and the Tracker, against a stand-in server. */
class TrackerTest {
    private val keys = Secrets(traktClientId = "tid", simklClientId = "sid")

    private fun client(web: FakeWeb, name: String) = TrackingClient(keys, web.http, "https://$name.trakt.test", "https://$name.simkl.test")

    @Test
    fun aTraktCodeIsGivenAndApproved() = runBlocking {
        val web = FakeWeb()
        val client = client(web, "t1")
        web.answer("https://t1.trakt.test/oauth/device/code", """{"user_code": "ABCD1234", "verification_url": "https://trakt.tv/activate", "device_code": "dev", "interval": 0, "expires_in": 600}""")
        val code = client.begin(TrackingService.Trakt)
        assertTrue("never asks more than once a second", code.userCode == "ABCD1234" && code.interval == 1)
        web.answer("https://t1.trakt.test/oauth/device/token", """{"access_token": "tok", "refresh_token": "ref", "expires_in": 100, "created_at": 1}""")
        assertEquals("tok", client.poll(code, 0)?.token)
        web.answer("https://t1.trakt.test/oauth/device/token", "{}", status = 400)
        assertNull("not approved yet", client.poll(code, 0))
        web.answer("https://t1.trakt.test/oauth/device/token", "{}", status = 410)
        assertEquals(TrackingError.Expired(), runCatching { client.poll(code, 0) }.exceptionOrNull())
    }

    @Test
    fun aSimklCodeComesWithItsLinkFilledIn() = runBlocking {
        val web = FakeWeb()
        val client = client(web, "s1")
        web.answer(
            TrackingClient.simklUrl("https://s1.simkl.test", "/oauth2/device", "sid"),
            """{"user_code": "XY12", "verification_uri": "https://simkl.com/pin", "verification_uri_complete": "https://simkl.com/pin/XY12", "device_code": "d"}""",
        )
        val code = client.begin(TrackingService.Simkl)
        assertTrue(code.link == "https://simkl.com/pin/XY12" && code.expiresIn == 900)
        val token = TrackingClient.simklUrl("https://s1.simkl.test", "/oauth2/token", "sid")
        web.answer(token, """{"error": "authorization_pending"}""", status = 400)
        assertNull(client.poll(code, 0))
        web.answer(token, """{"error": "expired_token"}""", status = 400)
        assertEquals(TrackingError.Expired(), runCatching { client.poll(code, 0) }.exceptionOrNull())
    }

    @Test
    fun aRoundRenewsSendsAndBringsIn() = runBlocking {
        val web = FakeWeb().apply {
            answer("https://r1.trakt.test/oauth/token", """{"access_token": "new", "refresh_token": "ref2", "expires_in": 86400, "created_at": 900}""")
            answer("https://r1.trakt.test/sync/history", "{}")
            answer("https://r1.trakt.test/sync/watchlist/movies", """[{"movie": {"title": "Listed", "year": 2021, "ids": {"imdb": "tt5"}}}]""")
            answer("https://r1.trakt.test/sync/watchlist/shows", "[]")
            answer("https://r1.trakt.test/sync/watched/movies", """[{"movie": {"title": "Seen", "ids": {"imdb": "tt6"}}}]""")
            answer("https://r1.trakt.test/sync/watched/shows", "[]")
        }
        val near = TrackedAccount(TrackingService.Trakt, "old", "ref", 1000)
        val queued = listOf(
            Change(1, "watched", "movie", "tt7", "tt7", 0),
            Change(2, "watched", "series", "tt8", "", 0),
            Change(3, "watched", "movie", "srv:1:x", "", 0),
            Change(4, "watchlist_add", "movie", "tt9", "", 0),
        )
        val outcome = Sync.exchange(client(web, "r1"), listOf(near), queued, pull = true, now = 950)
        assertEquals(listOf("new"), outcome.renewed.map { it.token })
        // 1 sent, 2 nothing to send, 3 not trackable; 4 failed (no answer set up: 404).
        assertEquals(listOf(1L, 2L, 3L), outcome.sent)
        assertEquals(listOf("Trakt: the service answered with error 404"), outcome.problems)
        assertEquals(listOf("tt5"), outcome.pulls.first().watchlist?.map { it.imdb })
        assertEquals(listOf("tt6"), outcome.pulls.first().watched?.map { it.imdb })
        assertEquals(listOf("new"), Sync.keepRenewed(listOf(near), outcome.renewed).map { it.token })
    }

    @Test
    fun whatWasTakenOffAnAccountGoesHereToo() = runBlocking {
        val store = Store.inMemory()
        store.put(TrackedAccount.KEY, listOf(TrackedAccount(TrackingService.Trakt, "t")))
        val web = FakeWeb().apply {
            answer("https://p1.trakt.test/sync/watchlist/movies", """[{"movie": {"title": "Kept", "year": 2021, "ids": {"imdb": "tt1"}}}, {"movie": {"title": "Gone", "year": 2021, "ids": {"imdb": "tt2"}}}]""")
            answer("https://p1.trakt.test/sync/watchlist/shows", "[]")
            answer("https://p1.trakt.test/sync/watched/movies", """[{"movie": {"title": "Seen", "ids": {"imdb": "tt6"}}}]""")
            answer("https://p1.trakt.test/sync/watched/shows", "[]")
        }
        val tracker = Tracker(store, keys, web.http).apply {
            traktApi = "https://p1.trakt.test"
            now = { 1_767_323_045 }
        }
        tracker.run(pull = true)
        assertTrue(store.inWatchlist("movie", "tt2"))
        assertEquals(true, store.progress("movie", "tt6", "tt6")?.finished)
        // On another device tt2 left the Watchlist and tt6 was unmarked.
        web.answer("https://p1.trakt.test/sync/watchlist/movies", """[{"movie": {"title": "Kept", "year": 2021, "ids": {"imdb": "tt1"}}}]""")
        web.answer("https://p1.trakt.test/sync/watched/movies", "[]")
        tracker.run(pull = true)
        assertFalse(store.inWatchlist("movie", "tt2"))
        assertTrue(store.inWatchlist("movie", "tt1"))
        assertNull(store.progress("movie", "tt6", "tt6"))
    }

    @Test
    fun theTrackerQueuesSendsAndSaysHowItStands() = runBlocking {
        val store = Store.inMemory()
        store.put(TrackedAccount.KEY, listOf(TrackedAccount(TrackingService.Trakt, "t")))
        val web = FakeWeb()
        val tracker = Tracker(store, keys, web.http).apply {
            traktApi = "https://q1.trakt.test"
            afterAChangeMillis = 50
            now = { 1_767_323_045 }
        }
        assertEquals("Not synced yet. Everything is in step with Trakt.", tracker.status.text)
        // Not trackable: not queued.
        tracker.trackWatched("movie", "srv:1:x", "srv:1:x")
        assertTrue(store.queuedChanges().isEmpty())
        // Trackable, but the service fails: it stays queued and the problem shows.
        tracker.trackWatched("movie", "tt1", "tt1")
        assertEquals("Not synced yet. 1 change still to send to Trakt.", tracker.status.text)
        delay(500)
        assertEquals(1, store.queuedChanges().size)
        assertTrue(tracker.status.problem)
        assertTrue(tracker.status.text, tracker.status.text.endsWith("Last try: Trakt: the service answered with error 404. Haven tries again later."))
        // The service answers: sent, and in step.
        web.answer("https://q1.trakt.test/sync/history", "{}")
        tracker.run(pull = false)
        assertTrue(store.queuedChanges().isEmpty())
        assertTrue(tracker.status.text, tracker.status.text.endsWith(". Everything is in step with Trakt."))
        assertTrue(tracker.status.text.startsWith("Last synced "))
        // Switched off: nothing syncs, and nothing is queued.
        tracker.setSync(TrackingService.Trakt, on = false)
        assertEquals("Linked, but not syncing.", tracker.status.rows[0].detail)
        assertTrue(tracker.status.text.startsWith("Link Trakt or Simkl"))
        tracker.trackWatchlist("movie", "tt2", on = true)
        assertTrue(store.queuedChanges().isEmpty())
    }

    @Test
    fun linkingShowsTheCodeAndKeepsTheAccount() = runBlocking {
        val store = Store.inMemory()
        val web = FakeWeb().apply {
            answer("https://l1.trakt.test/oauth/device/code", """{"user_code": "CODE", "verification_url": "https://trakt.tv/activate", "device_code": "dev", "interval": 1}""")
            answer("https://l1.trakt.test/oauth/device/token", """{"access_token": "tok"}""")
        }
        val tracker = Tracker(store, keys, web.http).apply { traktApi = "https://l1.trakt.test" }
        assertEquals("Not linked. Link opens trakt.tv and shows a short code to approve there.", tracker.rowText(TrackingService.Trakt))
        val opened = mutableListOf<String>()
        tracker.openLink = { opened += it }
        tracker.connect(TrackingService.Trakt)
        repeat(40) { if (tracker.account(TrackingService.Trakt) == null) delay(100) }
        assertEquals(listOf("https://trakt.tv/activate"), opened)
        assertEquals("Linked to Trakt.", tracker.linkStatus)
        assertEquals(listOf("tok"), TrackedAccount.load(store).map { it.token })
        assertTrue(tracker.rowText(TrackingService.Trakt).startsWith("Linked:"))
        tracker.disconnect(TrackingService.Trakt)
        assertEquals("Unlinked from Trakt. Haven has forgotten the sign-in.", tracker.linkStatus)
        assertTrue(TrackedAccount.load(store).isEmpty())
        assertEquals("Not available in this copy of Haven yet (no Simkl app key).", Tracker(store, Secrets()).rowText(TrackingService.Simkl))
    }
}

/** Media-server titles linked to their IMDb id for Trakt and Simkl. */
class ImdbLinkTest {
    /** `srv:1:<hex of s|tt7631058|series>`, as a gathering server names it. */
    private val gathered = "srv:1:" + "s|tt7631058|series".toByteArray().joinToString("") { "%02x".format(it) }

    @Test
    fun aGatheredIdSpellsItsImdbId() {
        assertEquals("tt7631058", ImdbLink.of(gathered, null)?.imdb)
        assertNull(ImdbLink.of("tt7631058", null))
        assertNull(ImdbLink.of("srv:1:abcdef12", null))
        val detail = MetaDetail("srv:2:x", "series", "A Show", imdbId = "tt5", videos = listOf(Video("e1", season = 1, episode = 2)))
        assertEquals(ImdbLink("tt5", mapOf("e1" to (1 to 2))), ImdbLink.of("srv:2:x", detail))
    }

    @Test
    fun episodesAreSentAndShownUnderTheirNumbers() {
        val link = ImdbLink("tt1", mapOf("e1" to (1 to 1), "e2" to (1 to 2)))
        assertEquals("tt1" to "tt1:1:2", link.serviceIds("series", "e2"))
        assertNull(link.serviceIds("series", "unknown"))
        assertEquals("tt1" to "", link.serviceIds("series", ""))
        assertEquals("tt1" to "tt1", link.serviceIds("movie", "srv:1:x"))
        assertEquals(setOf("e1", "e2"), link.merged("series", "s", setOf("e1"), setOf("tt1:1:2", "tt1:5:5")))
        assertEquals(setOf("m"), link.merged("movie", "m", emptySet(), setOf("tt1")))
    }
}
