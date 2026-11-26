package net.haven.app.titles

import kotlinx.coroutines.runBlocking
import net.haven.app.core.Store
import net.haven.app.testing.FakeWeb
import net.haven.app.testing.Fixture
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/** The Rust build's tests of titles (haven-titles, haven-addons samples), ported. */
class TitlesTest {
    @Test
    fun readsACatalogueAndSkipsBrokenEntries() {
        val metas = Titles.parseCatalog(Fixture.catalog)
        assertEquals("entries with no id, or that are not objects, are left out", 4, metas.size)
        assertEquals("tt0000001", metas[0].id)
        assertEquals("https://images.example/poster/tt0000001.jpg", metas[0].poster)
        assertEquals("2026", metas[0].releaseInfo)
        assertEquals("7.2", metas[0].rating)
        assertEquals("a year sent as a number", "2024", metas[1].releaseInfo)
        assertNull(metas[2].poster)
        assertEquals("series", metas[3].kind)
    }

    @Test
    fun anEmptyAnswerIsAnEmptyCatalogue() {
        assertTrue(Titles.parseCatalog("{}").isEmpty())
        assertTrue(Titles.parseCatalog("""{"metas":[]}""").isEmpty())
        assertThrows(TitlesError::class.java) { Titles.parseCatalog("nope") }
    }

    @Test
    fun readsDetailsWithOldAndNewSpellings() {
        val meta = Titles.parseMeta(Fixture.meta)
        assertEquals("Long Runner", meta.name)
        assertEquals("a rating sent as a number", "8.4", meta.rating)
        assertEquals("the current spelling wins", listOf("Drama", "Crime"), meta.genres)
        assertEquals(listOf("Ada Example", "Ben Sample"), meta.cast)
        assertTrue("null is an empty list", meta.director.isEmpty())
        assertEquals("49 min", meta.runtime)
        assertEquals("entries with no id, or not objects, are left out", 4, meta.videos.size)
        assertEquals("2020-03-01T05:00:00.000Z", meta.videos[0].released)
    }

    @Test
    fun ordersSeasonsAndEpisodes() {
        val meta = Titles.parseMeta(Fixture.meta)
        assertEquals("specials go last", listOf(1, 2, 0), meta.seasons)
        val first = meta.episodes(1).map { it.episode to it.title }
        assertEquals(listOf(1 to "Pilot", 2 to "Second"), first)
        assertEquals("season sent as text", "tt0000004:0:1", meta.episodes(0)[0].id)
    }

    @Test
    fun aMissingTitleIsNotAParseError() {
        assertThrows(TitlesError.UnknownTitle::class.java) { Titles.parseMeta("{}") }
        assertThrows(TitlesError.UnknownTitle::class.java) { Titles.parseMeta("""{"meta":{}}""") }
        assertThrows(TitlesError.UnknownTitle::class.java) { Titles.parseMeta("""{"meta":null}""") }
        val bad = assertThrows(TitlesError.BadResponse::class.java) { Titles.parseMeta("nope") }
        assertEquals("not JSON", bad.why)
    }

    @Test
    fun readsStreamsAndTellsHowEachIsDelivered() {
        val streams = MediaStream.parseList(Fixture.streams)
        assertEquals(
            "entries that name no source are left out",
            listOf(StreamKind.Direct, StreamKind.Direct, StreamKind.External, StreamKind.YouTube, StreamKind.PeerToPeer),
            streams.map { it.kind },
        )
        assertEquals("Direct file, 1.2 GB", streams[0].details)
        assertEquals("Older add-ons put the text here", streams[1].details)
        assertTrue(MediaStream.parseList("{}").isEmpty())
    }

    @Test
    fun aStreamSizeHintIsReadAsNumberOrText() {
        val json = """{"streams":[{"url":"https://a/1","behaviorHints":{"videoSize":123}},""" +
            """{"url":"https://a/2","behaviorHints":{"videoSize":"456"}},{"url":"https://a/3","behaviorHints":{"videoSize":0}}]}"""
        assertEquals(listOf(123L, 456L, null), MediaStream.parseList(json).map { it.videoSize })
    }

    @Test
    fun repeatsOfATitleAreDroppedAndTheCleanNameWins() {
        assertEquals(
            Titles.titleKey("movie", "Dune: Part Two", "2024"),
            Titles.titleKey("movie", "Dune Part Two (2024) [1080p] [WEBRip] 88", null),
        )
        assertNotEquals(Titles.titleKey("movie", "Dune", "2021"), Titles.titleKey("movie", "Dune", "1984"))
        assertNotEquals(Titles.titleKey("movie", "Dune", "2021"), Titles.titleKey("series", "Dune", "2021"))
        assertEquals("movie|duneparttwo|2024", Titles.titleKey("movie", "Dune: Part Two", "2024"))
        fun title(id: String, kind: String, name: String, year: String?) = MetaPreview(id, kind, name, releaseInfo = year)
        val kept = Titles.withoutRepeats(
            listOf(
                title("a", "movie", "Dune Part Two (2024) [1080p] [WEBRip] 88", null),
                title("b", "movie", "The Odyssey", "2026"),
                title("c", "movie", "Dune: Part Two", "2024"),
                title("d", "movie", "The Odyssey", "2026"),
                title("e", "movie", "Dune", "2021"),
                title("f", "series", "The Odyssey", "2026"),
            ),
        )
        assertEquals(listOf("c", "b", "e", "f"), kept.map { it.id })
    }

    private fun catalog(kind: String, id: String) = Catalog(kind, id)

    @Test
    fun cataloguesSitNextToTheManifest() {
        assertEquals(
            "https://service.example/catalog/movie/top.json",
            ResourceUrl.catalog("https://service.example/manifest.json", catalog("movie", "top")),
        )
        assertEquals(
            "https://service.example/cfg=a%7Cb/catalog/series/new%20shows%2F1.json",
            ResourceUrl.catalog("https://service.example/cfg=a%7Cb/manifest.json", catalog("series", "new shows/1")),
        )
    }

    @Test
    fun searchTermsAreFullyEncoded() {
        assertEquals(
            "https://service.example/catalog/movie/top/search=tom%20%26%20jerry%3A%20100%25%20caf%C3%A9%3F.json",
            ResourceUrl.catalog("https://service.example/manifest.json", catalog("movie", "top"), search = "tom & jerry: 100% café?"),
        )
    }

    @Test
    fun detailsAndStreamsUseTheTitleId() {
        val manifest = "https://service.example/manifest.json"
        assertEquals("https://service.example/meta/series/tt0000004.json", ResourceUrl.make(manifest, "meta", "series", "tt0000004"))
        assertEquals(
            "https://service.example/stream/series/tt0000004%3A1%3A2.json",
            ResourceUrl.make(manifest, "stream", "series", "tt0000004:1:2"),
        )
        assertEquals(
            "https://service.example/catalog/movie/top/genre=Sci-Fi&skip=100.json",
            ResourceUrl.make(manifest, "catalog", "movie", "top", listOf("genre" to "Sci-Fi", "skip" to "100")),
        )
        assertThrows(TitlesError::class.java) { ResourceUrl.make("https://service.example/other.json", "meta", "movie", "x") }
    }

    @Test
    fun cinemetaGivesDetailsForImdbIdsOnly() {
        val source = Cinemeta.source()
        assertTrue(source.provides("meta", "movie", "tt0111161"))
        assertTrue(source.provides("meta", "series", "tt0903747"))
        assertFalse(source.provides("meta", "movie", "tmdb:603"))
        assertFalse(source.provides("stream", "movie", "tt0111161"))
        assertEquals(4, source.catalogs.size)
        assertTrue(source.catalogs[0].searchable)
        assertEquals(listOf("Popular films", "Popular series", "Featured films", "Featured series"), source.catalogs.map { it.title })
        assertTrue(source.catalogs.all { !it.needsInput && it.pages })
    }

    @Test
    fun cinemetaKeepsItsLastDescription() = runBlocking {
        val store = Store.inMemory()
        assertEquals(Cinemeta.source(), Cinemeta.kept(store))
        val described = """{"id":"com.linvo.cinemeta","name":"Cinemeta","catalogs":[{"type":"movie","id":"top","name":"Popular","extra":[{"name":"search"}]}]}"""
        val web = FakeWeb().apply { answer(Cinemeta.URL, described) }
        val catalogues = Catalogues(web.http)
        val fresh = Cinemeta.refresh(store, Cinemeta.source(), catalogues)!!
        assertEquals(listOf("Popular films"), fresh.catalogs.map { it.title })
        assertEquals(fresh, Cinemeta.kept(store))
        // Asked again with nothing new: nothing changes.
        assertNull(Cinemeta.refresh(store, fresh, catalogues))
    }
}
