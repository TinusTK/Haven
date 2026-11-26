package net.haven.app.app

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import net.haven.app.core.AppPaths
import net.haven.app.core.Backup
import net.haven.app.core.Keys
import net.haven.app.core.Secrets
import net.haven.app.core.Settings
import net.haven.app.core.Store
import net.haven.app.core.StoredServer
import net.haven.app.core.SwitchedOff
import net.haven.app.core.Theme
import net.haven.app.core.TvSource
import net.haven.app.core.WatchProgress
import net.haven.app.library.Library
import net.haven.app.livetv.LiveTVModel
import net.haven.app.metadata.WhereToWatch
import net.haven.app.pictures.Kept
import net.haven.app.player.PlayerRules
import net.haven.app.testing.FakeWeb
import net.haven.app.tracking.TrackedAccount
import net.haven.app.tracking.Tracker
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.time.Instant
import java.time.ZoneOffset
import java.util.concurrent.Executors

private fun json(text: String): JsonObject = Json.parseToJsonElement(text).jsonObject

/** The Rust build's backup (1) and setup (3) tests (`haven-core`), ported
 *  from the iPhone's, and a backup the Rust build wrote. */
class BackupTest {
    private fun store() = Store.inMemory()

    private fun progress(video: String, position: Double, at: Long) =
        WatchProgress("series", "tt1", video, "A Show", "", null, position, 100.0, false, at)

    private fun server(name: String = "Den") = StoredServer(kind = "jellyfin", name = name, address = "http://192.168.1.9:8096", userId = "u", token = "t")

    @Test fun aBackupRoundTripsThroughAFile() {
        val from = store()
        from.saveServer(server())
        from.putRaw("settings", """{"theme":"light","last_page":2}""")
        from.put("device_id", "abc")
        val backup = Backup.make(from, sources = true, now = 100)
        assertNull("a device's own settings stay with it", (backup["settings"] as JsonObject)["device_id"])

        val folder = Files.createTempDirectory("haven-backup").toFile()
        val file = File(File(folder, "sub"), Backup.FILE_NAME)
        Backup.write(Backup.text(backup), file)
        Backup.write(Backup.text(backup), file) // replacing one works too
        val read = Backup.read(file.readText())
        assertEquals(Backup.contents(backup), Backup.contents(read))
        assertEquals(100L, Backup.madeAt(read))
        assertTrue(runCatching { Backup.read("""{"something": "else"}""") }.isFailure)
        assertTrue(runCatching { Backup.read("word") }.isFailure)

        val to = store()
        val merged = Backup.restore(read, to, sources = true)
        assertEquals(1, merged.added)
        assertEquals("Den", to.servers().first().name)
        assertEquals(Theme.Light, Settings.load(to).theme)
        folder.deleteRecursively()
    }

    @Test fun aSetupGoesFromOneStoreToAnotherAndNewerProgressWins() {
        val from = store()
        from.saveServer(StoredServer(kind = "jellyfin", name = "Den", address = "http://192.168.1.9:8096", userId = "u", token = "secret"))
        from.saveAddon("https://one.example/manifest.json", """{"id":"x","name":"One"}""")
        from.saveProgress(progress("tt1:1:1", 50.0, 20))
        from.saveProgress(progress("tt1:1:2", 10.0, 5))
        val to = store()
        to.saveProgress(progress("tt1:1:2", 70.0, 30))

        // Through JSON text and back, as a file carries it.
        val setup = json(Backup.text(Backup.setup(from, sources = true)))
        // Without add-ons: they are neither sent nor taken.
        assertTrue((Backup.setup(from, sources = false)["sources"] as JsonArray).isEmpty())
        Backup.merge(setup, to, sources = false)
        assertTrue(to.addons().isEmpty())
        assertEquals(1, to.servers().size)
        assertEquals(50.0, to.progress("series", "tt1", "tt1:1:1")!!.position, 0.0)
        // Its own newer position stays.
        assertEquals(70.0, to.progress("series", "tt1", "tt1:1:2")!!.position, 0.0)
        // A second time adds nothing.
        assertEquals(0, Backup.merge(setup, to, sources = false).added)
        Backup.merge(setup, to, sources = true)
        assertEquals(1, to.addons().size)
    }

    @Test fun theNewerTrackingSignInWins() {
        val store = store()
        store.putRaw(TrackedAccount.KEY, """[{"service":"Trakt","token":"old","expires_at":10}]""")
        val setup = json(
            """{"tracking": [{"service": "Trakt", "token": "new", "expires_at": 20}, {"service": "Simkl", "token": "s"}],
               "progress": [{"kind": "movie", "title_id": "tt9", "video_id": "tt9", "title": "A Film", "detail": "", "poster": null,
                             "position": 0.0, "duration": 0.0, "finished": true, "updated_at": 1}]}""",
        )
        val merged = Backup.merge(setup, store, sources = false)
        assertTrue(merged.tracking)
        assertTrue(store.progress("movie", "tt9", "tt9")!!.finished)
        val kept = Json.parseToJsonElement(store.rawValue(TrackedAccount.KEY)!!) as JsonArray
        assertEquals(2, kept.size)
        assertTrue(kept.any { it.jsonObject["token"]?.jsonPrimitive?.content == "new" })
        // An older one does not replace it.
        assertFalse(Backup.merge(json("""{"tracking": [{"service": "Trakt", "token": "old", "expires_at": 10}]}"""), store, sources = false).tracking)
    }

    @Test fun whatArrivesSwitchedOffStaysOff() {
        val setup = json(
            """{"servers": [{"kind": "plex", "name": "Den", "address": "http://den", "user_id": "u", "token": "t"}],
               "tv_sources": [{"url": "http://tv", "name": "TV"}],
               "off": {"servers": [{"kind": "plex", "address": "http://den"}], "addons": [], "tv_sources": ["http://tv"]}}""",
        )
        val store = store()
        Backup.merge(setup, store, sources = false)
        val off = SwitchedOff.load(store)
        assertEquals(listOf(store.servers()[0].id), off.servers)
        assertEquals(listOf("http://tv"), off.tvSources)
    }

    /** A backup as the Rust build writes it (serde's pretty JSON). */
    private val rustBackup = """
        {
          "kind": "Haven backup",
          "version": 1,
          "made_at": 1764000000,
          "setup": {
            "servers": [
              { "kind": "emby", "name": "Living room", "address": "http://192.168.1.20:8096", "user_id": "abc", "token": "tok" }
            ],
            "sources": [],
            "tv_sources": [ { "url": "https://tv.example/list.m3u", "name": "tv.example" } ],
            "keys": { "tmdb": "k1", "trakt_id": "", "trakt_secret": "", "simkl_id": "", "omdb": "" },
            "watchlist": [
              { "kind": "series", "title_id": "tt0944947", "title": "Game of Thrones", "year": "2011–2019", "poster": null, "added_at": 1700000000000 }
            ],
            "progress": [
              {
                "kind": "movie", "title_id": "tt0133093", "video_id": "tt0133093", "title": "The Matrix", "detail": "",
                "poster": "https://images.metahub.space/poster/medium/tt0133093/img",
                "position": 1234.5, "duration": 8160.0, "finished": false, "updated_at": 1700000000001
              }
            ],
            "tracking": [],
            "off": { "servers": [], "addons": [], "tv_sources": [] }
          },
          "settings": {
            "country": "AU",
            "player": { "volume": 0.8, "muted": false, "subtitle_scale": 1.2, "passthrough": false }
          }
        }
    """.trimIndent()

    @Test fun aBackupTheRustBuildWroteIsRestored() {
        val backup = Backup.read(rustBackup)
        assertEquals(1_764_000_000L, Backup.madeAt(backup))
        val store = store()
        val merged = Backup.restore(backup, store, sources = false)
        assertEquals(4, merged.added)
        assertEquals("Living room", store.servers().first().name)
        assertEquals("Game of Thrones", store.watchlist().first().title)
        assertNull(store.watchlist().first().poster)
        assertEquals(1234.5, store.progress("movie", "tt0133093", "tt0133093")!!.position, 0.0)
        assertEquals("k1", Keys.load(store).tmdb)
        assertEquals("AU", WhereToWatch.chosenCountry(store))
        assertEquals(1.2, PlayerRules.Preferences.load(store).subtitleScale, 0.0)
        // And what Haven writes back has the same names.
        val again = Backup.text(Backup.make(store, sources = false, now = 1))
        for (name in listOf("\"made_at\"", "\"user_id\"", "\"title_id\"", "\"added_at\"", "\"tv_sources\"", "\"subtitle_scale\"", "\"Haven backup\"")) {
            assertTrue("$name is written", again.contains(name))
        }
        assertFalse("slashes are written plainly", again.contains("\\/"))
    }

    @Test fun whenIsSaidPlainly() {
        val now = Instant.ofEpochSecond(1_764_072_000) // 25 Nov 2025 12:00 UTC
        assertEquals("today at 10:30", Backup.`when`(1_764_066_600, now, ZoneOffset.UTC))
        assertEquals("3 Nov at 10:30", Backup.`when`(1_762_165_800, now, ZoneOffset.UTC))
    }
}

/** Settings' own model. */
class SettingsModelTest {
    private val thread = Executors.newSingleThreadExecutor().asCoroutineDispatcher()
    private val scopes = mutableListOf<CoroutineScope>()

    @After fun stop() = scopes.forEach { it.cancel() }

    private fun scope() = CoroutineScope(SupervisorJob() + thread).also { scopes += it }
    private fun <T> on(block: () -> T): T = runBlocking(thread) { block() }

    private class Made(val settings: SettingsModel, val library: Library, val store: Store, val folder: File)

    private fun model(addons: Boolean = true): Made {
        val folder = Files.createTempDirectory("haven-settings").toFile()
        val store = Store.open(File(folder, "haven.sqlite3"))
        val scope = scope()
        val library = Library(store, Secrets(), FakeWeb().http, Kept(Files.createTempDirectory("haven-kept").toFile()), scope)
        val settings = SettingsModel(store, AppPaths.at(folder), library, Tracker(store, Secrets()), LiveTVModel(store, scope), scope, addons)
        settings.backupFolder = { File(folder, "Documents/Haven") to "Documents › Haven on this phone" }
        return Made(settings, library, store, folder)
    }

    private fun waitFor(test: () -> Boolean) {
        val until = System.currentTimeMillis() + 5_000
        while (!on(test)) {
            check(System.currentTimeMillis() < until) { "timed out" }
            Thread.sleep(10)
        }
    }

    @Test fun keysAreSavedAndUsed() {
        val made = model()
        on {
            assertNull(made.library.tmdb)
            made.settings.keys = made.settings.keys.copy(tmdb = "  mykey ")
            made.settings.saveKeys()
            assertEquals("Keys saved.", made.settings.keysStatus)
            assertEquals("  mykey ", Keys.load(made.store).tmdb)
            assertEquals("mykey", made.library.keys.tmdbApiKey)
        }
    }

    @Test fun generalChoicesAreKept() {
        val settings = model().settings
        on {
            settings.setCountry(1)
            assertEquals(1, settings.country)
            settings.setCountry(0)
            assertEquals(0, settings.country)
            settings.setOtherPlayer(1)
            assertEquals(1, settings.otherPlayer)
            settings.setOtherPlayer(0)
            assertEquals(0, settings.otherPlayer)
            settings.setPassthrough(true)
            settings.setLoud(true)
            assertTrue(settings.passthrough && settings.loud)
        }
    }

    @Test fun theListFollowsTheBuild() {
        for (addons in listOf(true, false)) {
            val sections = SettingsModel.Section.groups(addons).flatMap { it.second }
            assertEquals(addons, SettingsModel.Section.Addons in sections)
            assertEquals(SettingsModel.Section.General, sections.first())
            assertEquals(SettingsModel.Section.About, sections.last())
        }
    }

    @Test fun liveTVSourcesAreAddedAndRemoved() {
        val made = model()
        val settings = made.settings
        val address = "https://tv.settings.example/list.m3u"
        on {
            settings.playlistAddress = "not an address"
            settings.addPlaylist()
            assertTrue(settings.tvStatus!!.isError)
            made.store.saveTvSource(TvSource(address, "tv.settings.example", "#EXTM3U\n#EXTINF:-1,One\nhttps://tv.settings.example/one.m3u8\n"))
            assertEquals(listOf("tv.settings.example"), settings.tvSources.map { it.name })
            settings.setTVSource(address, false)
            assertFalse(settings.tvSources.first().enabled)
            settings.removeTVSource(address)
            assertTrue(settings.tvSources.isEmpty())
            assertEquals("Source removed.", settings.tvStatus?.text)
        }
    }

    @Test fun aBackupIsWrittenAndRestoredElsewhere() {
        val made = model()
        val settings = made.settings
        on {
            settings.now = { 1_000 }
            made.store.saveServer(StoredServer(kind = "jellyfin", name = "Den", address = "http://den", userId = "u", token = "t"))
            assertEquals(Backup.FIRST_NOTE, run { settings.startBackups(); settings.backupStatus })
            settings.backUp()
        }
        waitFor { !made.settings.backupBusy }
        assertTrue(File(made.folder, "Documents/Haven/${Backup.FILE_NAME}").isFile)
        on { assertTrue(settings.backupStatus, settings.backupStatus.endsWith("to Documents › Haven on this phone.")) }

        // Another device with no backup yet, then reading this one's.
        val other = model()
        other.settings.backupFolder = { File(other.folder, "nowhere") to "Documents › Haven on this phone" }
        on { other.settings.restore() }
        waitFor { !other.settings.backupBusy }
        on { assertEquals("There is no backup in Documents › Haven yet.", other.settings.backupStatus) }
        other.settings.backupFolder = settings.backupFolder
        on { other.settings.restore() }
        waitFor { !other.settings.backupBusy && other.settings.backupStatus.startsWith("Restored") }
        on {
            assertEquals("Den", other.store.servers().single().name)
            assertTrue(other.settings.backupStatus, other.settings.backupStatus.contains(": 1 new thing."))
        }
    }
}
