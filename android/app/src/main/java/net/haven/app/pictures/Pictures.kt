package net.haven.app.pictures

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import net.haven.app.core.Http
import net.haven.app.core.Log
import java.io.File

/** The sizes Haven shrinks pictures to, in pixels: a picture is fitted
 *  inside the box (never made larger). */
enum class PictureSize(val width: Int, val height: Int) {
    Poster(300, 450),
    Background(1600, 900),
    Logo(600, 240),
    Wide(640, 360);

    companion object {
        /** The size a `width`×`height` picture is shown at inside `box`. */
        fun fit(width: Int, height: Int, box: PictureSize): Pair<Int, Int> {
            val scale = minOf(box.width.toDouble() / width, box.height.toDouble() / height, 1.0)
            return maxOf(1, Math.round(width * scale).toInt()) to maxOf(1, Math.round(height * scale).toInt())
        }
    }
}

/** Reads and shrinks pictures: Android's own decoder in the app
 *  ([AndroidPictures]), a stand-in in the tests. */
interface PictureCodec<T : Any> {
    /** A picture's size in pixels; null if the bytes are not a picture. */
    fun size(bytes: ByteArray): Pair<Int, Int>?

    /** The picture fitted inside `box`; null if it cannot be read. */
    fun shrink(bytes: ByteArray, box: PictureSize): T?

    /** Memory a picture takes, roughly, for the in-memory cache. */
    fun cost(image: T): Int
}

/** Holds back work while it is shut (Home's shelves and their pictures wait
 *  while Home is not on screen). Looked at again every 2 s, as the Rust build
 *  did. */
class Pause {
    @Volatile var isShut = false
        private set

    fun set(shut: Boolean) {
        isShut = shut
    }

    /** Returns once the pause is open. */
    suspend fun pass(everyMillis: Long = 2_000) {
        while (isShut) delay(everyMillis)
    }
}

/** Fetches pictures a few at a time, shrinks them, and keeps them on disk:
 *  the next time one is wanted it comes from there. Failures are silent (the
 *  place for the picture stays empty) and noted in the log without the
 *  address's secrets. */
class Pictures<T : Any>(
    val folder: File,
    private val codec: PictureCodec<T>,
    private val http: Http = Http.shared,
) {
    companion object {
        /** The most a picture may take. */
        const val MAX_BYTES = 6 * 1024 * 1024
        /** A picture larger than this either way is not read. */
        const val MAX_SIDE = 8000
        const val AT_ONCE = 6
        const val MEMORY_BYTES = 64 * 1024 * 1024
    }

    private val gate = Semaphore(AT_ONCE)

    /** Pictures shrunk this run, so a picture seen again needs no reading
     *  (least recently used go first). */
    private val memory = object : LinkedHashMap<String, T>(64, 0.75f, true) {}
    private var memoryBytes = 0

    /** The file a picture is kept in: named after its address (without what
     *  follows `?`, so a new sign-in token does not make it a new picture) and
     *  the size asked for. */
    fun file(url: String, size: PictureSize) = File(folder, Cache.hashName("${Log.plainAddress(url)}|${size.name.lowercase()}"))

    /** Reads a picture and shrinks it to fit `size`; null if it is not one, or
     *  is larger than 8000 pixels either way. */
    fun shrink(bytes: ByteArray, size: PictureSize): T? {
        val (width, height) = codec.size(bytes) ?: return null
        if (width <= 0 || height <= 0 || width > MAX_SIDE || height > MAX_SIDE) return null
        return codec.shrink(bytes, size)
    }

    /** A picture, from memory, the disk, or the network; null if it cannot be
     *  had. `pause` holds the work back while shut. */
    suspend fun load(url: String, size: PictureSize, pause: Pause? = null): T? {
        val file = file(url, size)
        synchronized(memory) { memory[file.name] }?.let { return it }
        pause?.pass()
        return try {
            val image = gate.withPermit { fetch(url, size, file) }
            remember(file.name, image)
            image
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Log.note("picture not shown: ${Log.plainAddress(url)} (${e.message})")
            null
        }
    }

    private fun remember(key: String, image: T) = synchronized(memory) {
        memory.put(key, image)?.let { memoryBytes -= codec.cost(it) }
        memoryBytes += codec.cost(image)
        val keys = memory.keys.iterator()
        while (memoryBytes > MEMORY_BYTES && keys.hasNext()) {
            val oldest = keys.next()
            if (oldest == key) continue
            memory[oldest]?.let { memoryBytes -= codec.cost(it) }
            keys.remove()
        }
    }

    private suspend fun fetch(url: String, size: PictureSize, file: File): T {
        if (file.exists()) {
            runCatching { file.readBytes() }.getOrNull()?.let { shrink(it, size) }?.let { return it }
            // A damaged file: fetch the picture again.
            file.delete()
        }
        val bytes = http.getBytes(url, MAX_BYTES)
        val image = shrink(bytes, size)
            ?: throw IllegalStateException(
                "not a picture Haven can read; ${bytes.size} bytes starting ${bytes.take(8).joinToString(" ") { "%02x".format(it) }}",
            )
        // Kept only once it is known to be a picture, whole or not at all.
        runCatching {
            folder.mkdirs()
            val part = File(folder, file.name + ".part")
            part.writeBytes(bytes)
            if (!part.renameTo(file)) {
                file.delete()
                part.renameTo(file)
            }
        }
        return image
    }

    // Settings › Downloads & storage

    /** How many pictures are kept, and how much room they take. */
    fun usage(): Pair<Int, Long> {
        val files = folder.listFiles()?.filter { it.isFile && !it.name.endsWith(".part") } ?: emptyList()
        return files.size to files.sumOf { it.length() }
    }

    /** The line Settings shows. */
    fun usageText(device: String = "phone"): String {
        val (count, bytes) = usage()
        if (count == 0) return "No pictures are kept on this $device yet."
        return "$count picture${if (count == 1) "" else "s"} kept on this $device, ${"%.1f".format(java.util.Locale.ROOT, bytes / (1024.0 * 1024.0))} MB."
    }

    /** Deletes every kept picture and kept answer. Pictures on screen stay;
     *  they are fetched again the next time they are needed. */
    fun clear(kept: Kept) {
        kept.clear()
        folder.deleteRecursively()
        synchronized(memory) {
            memory.clear()
            memoryBytes = 0
        }
    }
}
