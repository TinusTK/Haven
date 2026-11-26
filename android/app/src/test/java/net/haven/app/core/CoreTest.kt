package net.haven.app.core

import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import okhttp3.mockwebserver.Dispatcher
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okhttp3.mockwebserver.RecordedRequest
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/** The Rust build's settings, secrets, paths, http and diag tests, ported
 *  as the Swift build has them. */
class SettingsTest {
    @Test
    fun defaultsWhenNothingSaved() {
        val store = Store.inMemory()
        assertEquals(Settings(), Settings.load(store))
        assertEquals(Theme.Dark, Settings().theme)
    }

    @Test
    fun savesAndLoads() {
        val store = Store.inMemory()
        val settings = Settings(Theme.Light, 3)
        settings.save(store)
        assertEquals(settings, Settings.load(store))
    }

    @Test
    fun typedKeysWinOverTheSecretsFile() {
        val store = Store.inMemory()
        assertEquals(Keys(), Keys.load(store))
        val keys = Keys(tmdb = " typed ")
        keys.save(store)
        val file = Secrets.parse("TMDB_API_KEY=filed\nSIMKL_CLIENT_ID=simkl")
        val resolved = Keys.load(store).resolved(file, builtIn = Secrets(simklClientId = "built"))
        assertEquals("typed", resolved.tmdbApiKey)
        assertEquals("simkl", resolved.simklClientId)
        assertFalse(keys.toString().contains("typed"))
    }

    @Test
    fun builtInKeysComeLastAndEmptyOnesCountAsMissing() {
        val resolved = Keys().resolved(Secrets(), builtIn = Secrets(traktClientId = "built", omdbApiKey = ""))
        assertEquals("built", resolved.traktClientId)
        assertNull(resolved.omdbApiKey)
    }

    @Test
    fun switchingOffIsRememberedAndUndone() {
        val store = Store.inMemory()
        var off = SwitchedOff.load(store)
        assertEquals(SwitchedOff(), off)
        off = off.copy(servers = SwitchedOff.switched(off.servers, 3L, on = false))
        off = off.copy(servers = SwitchedOff.switched(off.servers, 3L, on = false))
        off = off.copy(addons = SwitchedOff.switched(off.addons, "https://a.example/manifest.json", on = false))
        off.save(store)
        off = SwitchedOff.load(store)
        assertEquals(listOf(3L), off.servers)
        off = off.copy(servers = SwitchedOff.switched(off.servers, 3L, on = true))
        assertTrue(off.servers.isEmpty())
        assertEquals(1, off.addons.size)
    }

    @Test
    fun olderSavedSettingsStillLoad() {
        val store = Store.inMemory()
        store.putRaw(Settings.KEY, """{"theme":"light"}""")
        val loaded = Settings.load(store)
        assertEquals(Theme.Light, loaded.theme)
        assertEquals(0, loaded.lastPage)
    }

    @Test
    fun savedJsonMatchesTheRustBuild() {
        val store = Store.inMemory()
        SwitchedOff(addons = listOf("https://a/manifest.json"), servers = listOf(2), tvSources = emptyList()).save(store)
        val saved = Store.json.parseToJsonElement(store.rawValue(SwitchedOff.KEY)!!).jsonObject
        assertEquals(listOf("addons", "servers", "tv_sources"), saved.keys.sorted())
        // Rust's serde names, and slashes left alone.
        val keys = Store.json.encodeToString(Keys.serializer(), Keys(traktId = "x"))
        assertTrue(keys.contains("\"trakt_id\":\"x\""))
        assertFalse(store.rawValue(SwitchedOff.KEY)!!.contains("\\/"))
        assertTrue(Store.json.parseToJsonElement(keys) is JsonObject)
    }
}

class SecretsTest {
    @Test
    fun parsesCommentsQuotesAndExport() {
        val secrets = Secrets.parse(
            "# keys\nTMDB_API_KEY = abc123\nexport TRAKT_CLIENT_ID=\"id\"\n" +
                "TRAKT_CLIENT_SECRET='s=1'\nSIMKL_CLIENT_ID=\nUNKNOWN=x\nnot a line\r\nOMDB_API_KEY=o\r\n",
        )
        assertEquals("abc123", secrets.tmdbApiKey)
        assertEquals("id", secrets.traktClientId)
        assertEquals("s=1", secrets.traktClientSecret)
        assertNull(secrets.simklClientId)
        assertEquals("o", secrets.omdbApiKey)
    }

    @Test
    fun missingFileGivesEmptySecrets() {
        assertEquals(Secrets(), Secrets.fromFile(File("/nonexistent/haven-none.env")))
    }

    @Test
    fun printingHidesValues() {
        val shown = Secrets.parse("TMDB_API_KEY=topsecret").toString()
        assertFalse(shown.contains("topsecret"))
        assertTrue(shown.contains("set"))
    }
}

class PathsTest {
    @Test
    fun createsTheFolderAndNamesTheFiles() {
        val base = File(System.getProperty("java.io.tmpdir"), "haven-${System.nanoTime()}")
        try {
            val paths = AppPaths.at(File(base, "nested/haven"))
            assertTrue(paths.dataDir.isDirectory)
            assertEquals("haven.sqlite3", paths.database.name)
            assertEquals("haven.log", paths.logFile.name)
            assertEquals("secrets.env", paths.secretsFile.name)
        } finally {
            base.deleteRecursively()
        }
    }

    @Test
    fun theDeviceIdIsMadeOnceAndKept() {
        val store = Store.inMemory()
        val id = DeviceId.get(store)
        assertTrue(Regex("haven-[0-9a-f]{32}").matches(id))
        assertEquals(id, DeviceId.get(store))
    }
}

class LogTest {
    @Test
    fun loggedAddressesCarryNoSecrets() {
        assertEquals(
            "https://tv.example.org:8096/Items/1/Images/Primary",
            Log.plainAddress("https://me:secret@tv.example.org:8096/Items/1/Images/Primary?api_key=abc&tag=1"),
        )
        assertEquals("http://tv.example.org", Log.plainAddress("http://tv.example.org?x=1"))
        assertEquals("nonsense", Log.plainAddress("nonsense"))
    }

    @Test
    fun theLogStartsAfreshWhenTooBig() {
        val file = File.createTempFile("haven-", ".log").also { it.deleteOnExit() }
        file.writeBytes(ByteArray(Log.MAX_BYTES + 10) { 65 })
        Log.start(file, version = "test")
        assertTrue(file.length() < 1000)
        assertTrue(file.readText().contains("Haven test started"))
    }
}

class HttpTest {
    @Test
    fun acceptsOnlyWebAddresses() {
        assertEquals("https://example.com/a", Http.webUrl(" https://example.com/a ").toString())
        Http.webUrl("http://127.0.0.1:7000/manifest.json")
        assertThrows(HttpError.UnsupportedScheme::class.java) { Http.webUrl("file:///etc/passwd") }
        val bad = assertThrows(HttpError.InvalidUrl::class.java) { Http.webUrl("not an address") }
        assertEquals("not an address", bad.address)
    }

    private fun stub(): MockWebServer = MockWebServer().apply {
        dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse = when (request.path) {
                "/ok" -> MockResponse().setBody("hello")
                "/echo" -> MockResponse().setBody(
                    "${request.method} ${request.getHeader("User-Agent")} ${request.getHeader("Content-Type")} " +
                        "X-Test=${request.getHeader("X-Test")} ${request.body.readUtf8()}",
                )
                else -> MockResponse().setResponseCode(404).setBody("no")
            }
        }
        start()
    }

    @Test
    fun answersAreReadUpToTheLimit() = runBlocking {
        stub().use { server ->
            val http = Http()
            assertArrayEquals("hello".toByteArray(), http.getBytes(server.url("/ok").toString(), limit = 100))
            val big = runCatching { http.getBytes(server.url("/ok").toString(), limit = 4) }.exceptionOrNull()
            assertEquals(HttpError.TooLarge(4), big)
            val missing = runCatching { http.getBytes(server.url("/missing").toString(), limit = 100) }.exceptionOrNull()
            assertEquals(HttpError.Status(404), missing)
            // send() hands back an error's body with its status.
            val answer = http.send(server.url("/missing").toString(), limit = 100)
            assertTrue(answer.status == 404 && answer.text == "no")
        }
    }

    @Test
    fun sendsHavensNameAndTheBody() = runBlocking {
        stub().use { server ->
            val answer = Http().send(
                server.url("/echo").toString(), headers = listOf("X-Test" to "1"),
                body = "{\"a\":1}".toByteArray(), limit = 1000,
            )
            val echoed = answer.text
            assertTrue(echoed, echoed.contains("POST"))
            assertTrue(echoed, echoed.contains("Haven/"))
            assertTrue(echoed, echoed.contains("application/json"))
            assertTrue(echoed, echoed.contains("X-Test=1"))
            assertTrue(echoed, echoed.contains("{\"a\":1}"))
        }
    }
}
