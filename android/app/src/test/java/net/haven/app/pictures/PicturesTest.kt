package net.haven.app.pictures

import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.Serializable
import net.haven.app.core.AppPaths
import net.haven.app.metadata.Omdb
import net.haven.app.testing.FakeWeb
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.util.concurrent.atomic.AtomicInteger

/** A stand-in picture: the text "PIC <width>x<height>". */
data class FakeImage(val width: Int, val height: Int)

class FakeCodec : PictureCodec<FakeImage> {
    val busy = AtomicInteger()
    val most = AtomicInteger()
    var slowMillis = 0L

    override fun size(bytes: ByteArray): Pair<Int, Int>? {
        val m = Regex("^PIC (\\d+)x(\\d+)$").find(String(bytes)) ?: return null
        return m.groupValues[1].toInt() to m.groupValues[2].toInt()
    }

    override fun shrink(bytes: ByteArray, box: PictureSize): FakeImage? {
        val now = busy.incrementAndGet()
        most.accumulateAndGet(now) { a, b -> maxOf(a, b) }
        if (slowMillis > 0) Thread.sleep(slowMillis)
        busy.decrementAndGet()
        val (w, h) = size(bytes) ?: return null
        val (fw, fh) = PictureSize.fit(w, h, box)
        return FakeImage(fw, fh)
    }

    override fun cost(image: FakeImage) = image.width * image.height * 4
}

@Serializable
data class Shelf(val titles: List<String>, val page: Int)

class PicturesTest {
    private fun temp(): File = Files.createTempDirectory("haven-pictures").toFile()

    // Kept answers

    @Test fun keptRoundTrip() {
        val kept = Kept(temp())
        val name = Kept.shelf("home", "Popular", "cinemeta")
        assertNull(kept.read<Shelf>(name))
        kept.write(name, Shelf(listOf("a", "b"), 1))
        assertEquals(Shelf(listOf("a", "b"), 1), kept.read<Shelf>(name))
        assertTrue(kept.file(name).name.matches(Regex("[0-9a-f]{32}\\.json")))
        // Something unreadable is nothing.
        kept.file(name).writeText("not json")
        assertNull(kept.read<Shelf>(name))
    }

    @Test fun keptDetailsSurviveANewKept() {
        val dir = temp()
        Kept(dir).write(Kept.details("movie", "tt1"), Shelf(listOf("x"), 2))
        assertEquals(Shelf(listOf("x"), 2), Kept(dir).read<Shelf>(Kept.details("movie", "tt1")))
        assertNull(Kept(dir).read<Shelf>(Kept.details("series", "tt1")))
    }

    @Test fun freshenSaysWhenItChanged() {
        val kept = Kept(temp())
        val name = Kept.explore("Top", "movie", "")
        assertTrue(kept.freshen(name, Shelf(listOf("a"), 1)))
        assertFalse(kept.freshen(name, Shelf(listOf("a"), 1)))
        assertTrue(kept.freshen(name, Shelf(listOf("a", "b"), 1)))
        kept.clear()
        assertNull(kept.read<Shelf>(name))
    }

    @Test fun omdbScoresAreKeptAWeek() = runBlocking {
        val kept = Kept(temp())
        val url = "https://www.omdbapi.com/?i=tt0000099&apikey=abcd1234"
        val web = FakeWeb().apply { answer(url, """{"Response": "True", "imdbRating": "8.1"}""") }
        val first = kept.scores("tt0000099", "abcd1234", now = 1_000, http = web.http)
        assertEquals(Omdb.Scores(imdb = "8.1"), first)
        assertEquals(first, kept.scores("tt0000099", "abcd1234", now = 1_000 + 86_400, http = web.http))
        assertEquals(1, web.count(url))
        // A week on, asked again; a failure keeps the kept ones.
        web.answer(url, "", status = 500)
        assertEquals(first, kept.scores("tt0000099", "abcd1234", now = 1_000 + Kept.OMDB_KEPT_FOR, http = web.http))
        assertEquals(2, web.count(url))
        // No key: the kept ones.
        assertEquals(first, kept.scores("tt0000099", null, now = 1_000 + 30 * 86_400L, http = web.http))
    }

    @Test fun rustCacheRemovedOnce() {
        val paths = AppPaths.at(temp())
        File(paths.dataDir, "cache/pictures").mkdirs()
        File(paths.dataDir, "cache/pictures/x").writeText("old")
        File(paths.dataDir, "haven.sqlite3").writeText("data")
        Cache.removeRustCache(paths)
        assertFalse(File(paths.dataDir, "cache").exists())
        assertTrue(File(paths.dataDir, "haven.sqlite3").exists())
        Cache.removeRustCache(paths)
    }

    // Pictures

    @Test fun fitsInsideTheBoxNeverLarger() {
        assertEquals(300 to 450, PictureSize.fit(600, 900, PictureSize.Poster))
        assertEquals(200 to 300, PictureSize.fit(200, 300, PictureSize.Poster))
        assertEquals(300 to 150, PictureSize.fit(1000, 500, PictureSize.Poster))
        assertEquals(1600 to 900, PictureSize.fit(3840, 2160, PictureSize.Background))
        assertEquals(600 to 100, PictureSize.fit(1200, 200, PictureSize.Logo))
        assertEquals(640 to 360, PictureSize.fit(1280, 720, PictureSize.Wide))
        assertEquals(1 to 450, PictureSize.fit(1, 5000, PictureSize.Poster))
    }

    @Test fun fetchedOnceThenFromDisk() = runBlocking {
        val folder = temp()
        val web = FakeWeb().apply { answer("https://img.test/a.jpg", "PIC 600x900") }
        val first = Pictures(folder, FakeCodec(), web.http)
        assertEquals(FakeImage(300, 450), first.load("https://img.test/a.jpg", PictureSize.Poster))
        assertEquals(FakeImage(300, 450), first.load("https://img.test/a.jpg", PictureSize.Poster))
        assertEquals(1, web.count("https://img.test/a.jpg"))
        // A new run reads it from disk.
        val again = Pictures(folder, FakeCodec(), web.http)
        assertEquals(FakeImage(300, 450), again.load("https://img.test/a.jpg", PictureSize.Poster))
        assertEquals(1, web.count("https://img.test/a.jpg"))
        // Another size is another picture.
        assertEquals(FakeImage(600, 900), again.load("https://img.test/a.jpg", PictureSize.Background))
        assertEquals(2, web.count("https://img.test/a.jpg"))
    }

    @Test fun aNewTokenIsTheSamePicture() {
        val pictures = Pictures(temp(), FakeCodec(), FakeWeb().http)
        assertEquals(
            pictures.file("https://server.test/Items/1/Images/Primary?api_key=old", PictureSize.Poster),
            pictures.file("https://server.test/Items/1/Images/Primary?api_key=new", PictureSize.Poster),
        )
    }

    @Test fun damagedFileIsFetchedAgainAndNonPicturesAreNotKept() = runBlocking {
        val folder = temp()
        val web = FakeWeb().apply {
            answer("https://img.test/a.jpg", "PIC 100x100")
            answer("https://img.test/page.html", "<html>")
        }
        val pictures = Pictures(folder, FakeCodec(), web.http)
        val file = pictures.file("https://img.test/a.jpg", PictureSize.Poster)
        folder.mkdirs()
        file.writeText("garbage")
        assertEquals(FakeImage(100, 100), pictures.load("https://img.test/a.jpg", PictureSize.Poster))
        assertEquals("PIC 100x100", file.readText())
        assertNull(pictures.load("https://img.test/page.html", PictureSize.Poster))
        assertFalse(pictures.file("https://img.test/page.html", PictureSize.Poster).exists())
        // Too large either way is not read.
        web.answer("https://img.test/huge.jpg", "PIC 9000x10")
        assertNull(pictures.load("https://img.test/huge.jpg", PictureSize.Poster))
        // A failed fetch is silent.
        assertNull(pictures.load("https://img.test/missing.jpg", PictureSize.Poster))
    }

    @Test fun usageAndClear() = runBlocking {
        val folder = temp()
        val kept = Kept(temp())
        val web = FakeWeb().apply {
            answer("https://img.test/a.jpg", "PIC 10x10")
            answer("https://img.test/b.jpg", "PIC 20x20")
        }
        val pictures = Pictures(folder, FakeCodec(), web.http)
        assertEquals("No pictures are kept on this phone yet.", pictures.usageText())
        pictures.load("https://img.test/a.jpg", PictureSize.Poster)
        assertEquals("1 picture kept on this phone, 0.0 MB.", pictures.usageText())
        pictures.load("https://img.test/b.jpg", PictureSize.Poster)
        assertEquals(2 to 18L, pictures.usage())
        assertEquals("2 pictures kept on this TV, 0.0 MB.", pictures.usageText("TV"))
        kept.write("x", Shelf(emptyList(), 0))
        pictures.clear(kept)
        assertEquals(0 to 0L, pictures.usage())
        assertNull(kept.read<Shelf>("x"))
        // Cleared from memory too: fetched again.
        pictures.load("https://img.test/a.jpg", PictureSize.Poster)
        assertEquals(2, web.count("https://img.test/a.jpg"))
    }

    @Test fun sixAtOnce() = runBlocking {
        val web = FakeWeb()
        val codec = FakeCodec().apply { slowMillis = 50 }
        val pictures = Pictures(temp(), codec, web.http)
        val urls = (1..20).map { "https://img.test/$it.jpg" }.onEach { web.answer(it, "PIC 10x10") }
        kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
            urls.map { async { pictures.load(it, PictureSize.Poster) } }.awaitAll()
        }.forEach { assertNotNull(it) }
        assertTrue("at most ${codec.most.get()}", codec.most.get() in 2..Pictures.AT_ONCE)
    }

    @Test fun pauseHoldsWorkBack() = runBlocking {
        val pause = Pause().apply { set(true) }
        var passed = false
        val job = launch { pause.pass(everyMillis = 10); passed = true }
        delay(50)
        assertFalse(passed)
        pause.set(false)
        job.join()
        assertTrue(passed)
    }

    @Test fun lateAnswersAreDropped() {
        val generation = Generation()
        val first = generation.next()
        assertTrue(generation.isCurrent(first))
        val second = generation.next()
        assertFalse(generation.isCurrent(first))
        assertTrue(generation.isCurrent(second))
    }
}
