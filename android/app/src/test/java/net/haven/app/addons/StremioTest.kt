package net.haven.app.addons

import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import net.haven.app.core.Store
import net.haven.app.testing.FakeWeb
import net.haven.app.titles.Cinemeta
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/** The Rust build's Stremio tests (haven-addons/src/stremio.rs), ported. */
class StremioTest {
    private fun json(text: String) = Json.parseToJsonElement(text)

    @Test
    fun onAComputerOrGitHubBuildOnlyTheAccountsOwnAddOnsFollowIt() {
        val listed = { url: String -> Stremio.Listed(url, "{}") }
        val (add, remove, kept) = Stremio.followedChanges(listOf("mine", "both", "old"), listOf("old"), listOf(listed("both"), listed("new")))
        assertEquals(listOf("new"), add.map { it.url })
        // "old" came from the account and left it; "mine" was added by hand.
        assertEquals(listOf("old"), remove)
        // "both" was installed by hand first: signing out leaves it.
        assertEquals(listOf("new"), kept)
        // Signed out: only what the account brought in goes.
        val out = Stremio.followedChanges(listOf("mine", "new"), kept, emptyList())
        assertTrue(out.first.isEmpty() && out.third.isEmpty())
        assertEquals(listOf("new"), out.second)
    }

    @Test
    fun onGooglePlayTheWholeListIsTheAccounts() {
        val wanted = listOf(Stremio.Listed("https://a/manifest.json", "{}"), Stremio.Listed("https://b/manifest.json", "{}"))
        val (add, remove) = Stremio.changes(listOf("https://b/manifest.json", "https://old/manifest.json"), wanted)
        assertEquals(listOf("https://a/manifest.json"), add.map { it.url })
        assertEquals(listOf("https://old/manifest.json"), remove)
    }

    @Test
    fun aSignInGivesTheKeyAndEmail() {
        val account = Stremio.parseLogin(json("""{"authKey": "k1", "user": {"_id": "u", "email": "a@b.c"}}"""))
        assertTrue(account.email == "a@b.c" && account.authKey == "k1")
        assertFalse(account.toString().contains("k1"))
        assertThrows(Stremio.Failure.BadAnswer::class.java) { Stremio.parseLogin(json("""{"user": {}}""")) }
    }

    @Test
    fun theAccountIsKeptAsTheRustBuildKeepsIt() {
        val store = Store.inMemory()
        store.put(Stremio.ACCOUNT_KEY, Stremio.Account("a@b.c", "k1"))
        val saved = Json.parseToJsonElement(store.rawValue(Stremio.ACCOUNT_KEY)!!).jsonObject
        assertEquals(setOf("email", "auth_key"), saved.keys)
        assertEquals("a@b.c", Stremio.account(store)?.email)
    }

    @Test
    fun theListLeavesOutWhatHavenHasOrCannotReach() {
        val listed = Stremio.parseListed(
            json(
                """{"addons": [
                {"transportUrl": "${Cinemeta.URL}", "manifest": {"id": "com.linvo.cinemeta"}},
                {"transportUrl": "http://127.0.0.1:11470/local-addon/manifest.json", "manifest": {"id": "local"}},
                {"transportUrl": "https://aiostreams.example/u/manifest.json", "manifest": {"id": "aio", "name": "AIO"}},
                {"transportUrl": "https://broken.example/manifest.json"}
            ]}""",
            ),
        )
        assertEquals(1, listed.size)
        assertEquals("https://aiostreams.example/u/manifest.json", listed[0].url)
        assertTrue(listed[0].manifest.contains("\"AIO\""))
    }

    @Test
    fun anErrorAnswerSaysWhatStremioSaid() {
        val refused = assertThrows(Stremio.Failure.Refused::class.java) {
            Stremio.result("""{"error":{"message":"Wrong password"}}""".toByteArray())
        }
        assertEquals("Wrong password", refused.message)
        assertThrows(Stremio.Failure.BadAnswer::class.java) { Stremio.result("nope".toByteArray()) }
    }

    @Test
    fun signingInBringsTheAccountsAddOnsAndSigningOutTakesThemAway() = runBlocking {
        val store = Store.inMemory()
        val web = FakeWeb().apply {
            answer("${Stremio.API}/login", """{"result": {"authKey": "k1", "user": {"email": "a@b.c"}}}""")
            answer(
                "${Stremio.API}/addonCollectionGet",
                """{"result": {"addons": [{"transportUrl": "https://aio.example/manifest.json", "manifest": {"id": "aio", "name": "AIO"}}]}}""",
            )
        }
        val account = Stremio.login(" a@b.c ", "pw", web.http)
        val sent = Json.parseToJsonElement(web.sent("${Stremio.API}/login")!!) as JsonObject
        assertEquals("\"a@b.c\"", sent["email"].toString())
        store.put(Stremio.ACCOUNT_KEY, account)
        // GitHub build: one installed by hand stays when the account goes.
        store.saveAddon("https://mine.example/manifest.json", """{"id":"mine","name":"Mine"}""")
        assertTrue(Stremio.follow(Stremio.listed(account, web.http), store, wholeList = false))
        assertEquals(listOf("https://mine.example/manifest.json", "https://aio.example/manifest.json"), store.addons().map { it.url })
        assertFalse("nothing new: nothing changes", Stremio.follow(Stremio.listed(account, web.http), store, wholeList = false))
        assertTrue(Stremio.follow(emptyList(), store, wholeList = false))
        assertEquals(listOf("https://mine.example/manifest.json"), store.addons().map { it.url })
        assertNull(store.rawValue(Stremio.FOLLOWED_KEY))
    }
}
