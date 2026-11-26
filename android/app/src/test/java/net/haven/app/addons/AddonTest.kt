package net.haven.app.addons

import kotlinx.coroutines.runBlocking
import net.haven.app.core.Store
import net.haven.app.core.SwitchedOff
import net.haven.app.testing.FakeWeb
import net.haven.app.testing.Fixture
import net.haven.app.titles.Catalogues
import net.haven.app.titles.Cinemeta
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/** The Rust build's add-on tests (haven-addons samples and url), ported. */
class AddonTest {
    @Test
    fun readsAManifest() {
        val manifest = AddonManifest.parse(Fixture.manifest)
        assertEquals("org.example.sample", manifest.id)
        assertEquals("Sample Catalogue", manifest.name)
        assertEquals("1.2.0", manifest.version)
        assertEquals(listOf("movie", "series"), manifest.types)
        assertEquals(5, manifest.catalogs.size)
        val top = manifest.catalogs[0]
        assertTrue(top.kind == "movie" && top.id == "top")
        assertEquals("Popular films", top.title)
        assertFalse("optional extras do not block a shelf", top.needsInput)
        assertEquals(listOf("Action", "Drama"), top.genres)
        assertEquals("Channels", manifest.catalogs[4].title)
    }

    @Test
    fun cataloguesThatNeedAGenreOrIdsAreNotShelves() {
        val manifest = AddonManifest.parse(Fixture.manifest)
        assertEquals(listOf("top", "top", "picks"), manifest.catalogs.filter { !it.needsInput }.map { it.id })
        // "New" needs a genre but has genres: it can be browsed; "Last videos" cannot.
        assertEquals(listOf("top", "top", "year", "picks"), manifest.catalogs.filter { it.browsable }.map { it.id })
    }

    @Test
    fun badManifestsAreRefused() {
        assertThrows(AddonError::class.java) { AddonManifest.parse("<html>") }
        assertThrows(AddonError::class.java) { AddonManifest.parse("""{"name":"x"}""") }
        val incomplete = assertThrows(AddonError.IncompleteManifest::class.java) { AddonManifest.parse("""{"id":" ","name":"x"}""") }
        assertEquals("id", incomplete.field)
        assertTrue(AddonManifest.parse("""{"id":"a.b","name":"Tiny"}""").catalogs.isEmpty())
    }

    @Test
    fun knowsWhichCataloguesCanBeSearched() {
        val manifest = AddonManifest.parse(Fixture.manifest)
        assertEquals(listOf("movie/top"), manifest.catalogs.filter { it.searchable }.map { "${it.kind}/${it.id}" })
        assertEquals("Films", manifest.catalogs[0].resultsTitle)
    }

    @Test
    fun knowsWhatAnAddonCanBeAskedFor() {
        val manifest = AddonManifest.parse(Fixture.manifest)
        // "meta" is in the short form: the manifest's types and id prefixes apply.
        assertTrue(manifest.provides("meta", "movie", "tt0000001"))
        assertTrue(manifest.provides("meta", "series", "tt0000004"))
        assertFalse(manifest.provides("meta", "channel", "tt0000001"))
        assertFalse(manifest.provides("meta", "movie", "kitsu:1"))
        // "stream" is in the full form and only covers films.
        assertTrue(manifest.provides("stream", "movie", "tt0000001"))
        assertFalse(manifest.provides("stream", "series", "tt0000004:1:1"))
        assertFalse(manifest.provides("subtitles", "movie", "tt0000001"))
        val source = manifest.source("https://addon.example/manifest.json")
        assertTrue(source.provides("stream", "movie", "tt0000001"))
        assertFalse(source.provides("stream", "series", "tt0000004:1:1"))
    }

    @Test
    fun acceptsTheWaysPeoplePasteAnAddress() {
        val want = "https://addon.example/manifest.json"
        for (input in listOf(
            "https://addon.example/manifest.json", "  https://addon.example  ", "https://addon.example/",
            "stremio://addon.example/manifest.json", "https://addon.example/manifest.json#top",
        )) {
            assertEquals(input, want, AddonManifest.manifestUrl(input))
        }
        assertEquals("http://127.0.0.1:7000/cfg=a%7Cb/manifest.json", AddonManifest.manifestUrl("http://127.0.0.1:7000/cfg=a%7Cb"))
    }

    @Test
    fun refusesThingsThatAreNotWebAddresses() {
        for (input in listOf("", "   ", "addon", "ftp://addon.example", "file:///tmp/manifest.json")) {
            assertThrows(input, AddonError::class.java) { AddonManifest.manifestUrl(input) }
        }
    }

    @Test
    fun installingKeepsTheManifestAndCinemetaIsBuiltIn() = runBlocking {
        val store = Store.inMemory()
        val url = "https://addon.example/manifest.json"
        val web = FakeWeb().apply { answer(url, Fixture.manifest) }
        val client = AddonClient(Catalogues(web.http))
        assertEquals(InstalledAddons.Outcome.Installed("Sample Catalogue"), InstalledAddons.install("https://addon.example", store, client))
        assertEquals(InstalledAddons.Outcome.BuiltIn, InstalledAddons.install("https://v3-cinemeta.strem.io", store, client))
        val broken = InstalledAddons.install("https://missing.example", store, client)
        assertTrue(broken.isError)
        assertTrue(broken.message, broken.message.startsWith("Could not install the add-on: the server answered with error 404"))

        var installed = InstalledAddons.load(store)
        assertEquals(listOf("Sample Catalogue"), installed.rows.map { it.name })
        assertEquals(3, installed.rows[0].catalogs)
        assertEquals(1, installed.sources.size)
        // Switched off: listed, never asked.
        InstalledAddons.setEnabled(url, on = false, store = store)
        installed = InstalledAddons.load(store)
        assertTrue(!installed.rows[0].enabled && installed.sources.isEmpty())
        InstalledAddons.remove(url, store)
        assertTrue(InstalledAddons.load(store).rows.isEmpty())
        assertTrue(SwitchedOff.load(store).addons.isEmpty())
    }

    @Test
    fun anOldCinemetaAddonIsRemoved() {
        val store = Store.inMemory()
        store.saveAddon(Cinemeta.URL, """{"id":"com.linvo.cinemeta","name":"Cinemeta"}""")
        store.saveAddon("https://bad.example/manifest.json", "not json")
        assertTrue(InstalledAddons.load(store).rows.isEmpty())
        assertEquals("a broken one is kept, skipped", listOf("https://bad.example/manifest.json"), store.addons().map { it.url })
    }

    @Test
    fun streamsComeFromNextToTheManifest() = runBlocking {
        val web = FakeWeb().apply { answer("https://addon.example/stream/movie/tt0000001.json", Fixture.streams) }
        val streams = AddonClient(Catalogues(web.http)).streams("https://addon.example/manifest.json", "movie", "tt0000001")
        assertEquals(5, streams.size)
    }
}
