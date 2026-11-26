package net.haven.app.library

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import net.haven.app.core.Download
import net.haven.app.core.Http
import net.haven.app.core.Log
import net.haven.app.core.Store
import java.io.File
import java.io.FileOutputStream

/**
 * Films and episodes saved to this device, in Movies › Haven (`Movies/` and
 * `Series/<show>/Season <n>/`), and the Library's list of them. Two are
 * fetched at once; the rest wait. A paused download keeps what it has
 * (`<file>.part`) and carries on from there. Downloads go on while Haven is
 * open, as in the Rust build.
 */
class Downloads(
    private val store: Store,
    val root: File,
    private val scope: CoroutineScope,
    private val http: Http = Http.shared,
    /** Where downloads' pictures are kept (Haven's own folder), one a download. */
    private val pictures: File = File(root, ".pictures"),
    /** Where news of a download is handled (the main thread; another in tests). */
    private val newsOn: kotlin.coroutines.CoroutineContext? = null,
) {
    private val main get() = newsOn ?: Dispatchers.Main.immediate
    var rows by mutableStateOf(listOf<DownloadRow>())
        private set
    var count by mutableIntStateOf(0)
        private set
    /** Goes up whenever a download changes (the title page's buttons read it). */
    var revision by mutableIntStateOf(0)
        private set

    /** A finished file is shown to the device's other apps (Android's media list). */
    var saved: (File) -> Unit = {}
    /** Starts fetching one download; a stand-in in the tests. Calls
     *  `progress(size, received)` now and then and `ended(saved, size,
     *  received, why)` once, on the main thread. */
    var begin: (Download, progress: (Long, Long) -> Unit, ended: (Boolean, Long, Long, String?) -> Unit) -> Job? = { d, progress, ended ->
        scope.launch { fetch(d, progress, ended) }
    }

    private val running = HashMap<Long, Job>()
    /** Bytes so far of each running download (newer than the database). */
    private val live = HashMap<Long, Pair<Long, Long>>()
    /** Being paused (their end is not a failure). */
    private val pausing = HashSet<Long>()
    private var started = false

    /** Carries on with what waits and lists them all. */
    fun start() {
        if (started) return
        started = true
        // A download marked running when Haven was ended waits its turn again.
        for (d in all()) if (d.state == "loading") runCatching { store.updateDownload(d.id, d.size, d.received, "waiting") }
        next()
        show()
    }

    private fun all(): List<Download> = runCatching { store.downloads() }.getOrDefault(emptyList())

    // From the title page

    /** Whether the stream at `address` was downloaded: 0 no, 1 on its way, 2 saved. */
    fun savedState(address: String): Int {
        @Suppress("UNUSED_VARIABLE") val seen = revision
        val d = all().firstOrNull { it.url == address && it.state != "failed" } ?: return 0
        return if (d.state == "done") 2 else 1
    }

    /** What pressing a row's download button does. Says what happened, in
     *  words for the title page; `play` plays a saved copy. */
    fun download(request: PlayRequest, season: Int?, episode: Int?, picture: String? = null, play: (PlayRequest) -> Unit): String {
        val existing = all()
        // This one is downloaded already: play the saved copy (or say how far it has come).
        existing.firstOrNull { it.url == request.address && it.state != "failed" }?.let { saved ->
            if (saved.state == "done") {
                playRequest(saved)?.let(play)
                return ""
            }
            val (size, received) = live[saved.id] ?: (saved.size to saved.received)
            if (size <= 0) return "Still downloading. Press play to stream it now."
            return "Still downloading (${Math.round(received.toDouble() / size * 100)}%). Press play to stream it now."
        }
        val kind = if (season != null) "series" else request.kind
        // The same film or episode already saved or on its way.
        if (existing.any { it.kind == kind && it.videoId == request.videoId && it.state != "failed" }) {
            return "That one is already in your downloads (Library)."
        }
        var d = Download(0, kind, request.titleId, request.videoId, request.title, request.detail, season, episode, request.poster,
            request.address, "", 0, 0, "waiting", System.currentTimeMillis())
        d = d.copy(path = unusedPath(DownloadRules.relativePath(d, DownloadRules.extensionOf(d.url)), existing).path)
        val id = runCatching { store.addDownload(d) }.getOrElse {
            Log.note("could not add a download: ${it.message}")
            return "Could not start the download."
        }
        (picture ?: request.poster)?.let { keepPicture(id, it) }
        next()
        show()
        return "Downloading to ${DownloadRules.placeWords(root)}. It is in your Library under Downloads."
    }

    /** Fetches a new download's picture and keeps it with the download, so
     *  Downloads shows it offline too. */
    private fun keepPicture(id: Long, address: String) {
        scope.launch {
            val bytes = runCatching { http.getBytes(address, 6 * 1024 * 1024) }.getOrNull() ?: return@launch
            withContext(Dispatchers.IO) {
                runCatching {
                    pictures.mkdirs()
                    File(pictures, "$id.img").writeBytes(bytes)
                }
            }
            show()
        }
    }

    fun pictureFile(id: Long) = File(pictures, "$id.img")

    /** A path not used by another download (two sources of one title). */
    private fun unusedPath(relative: String, taken: List<Download>): File {
        val used = taken.map { it.path }.toSet()
        val wanted = File(root, relative)
        var candidate = wanted
        var number = 2
        while (candidate.path in used || candidate.exists()) {
            candidate = File(wanted.parentFile, "${wanted.nameWithoutExtension} ($number).${wanted.extension}")
            number += 1
        }
        return candidate
    }

    // Running

    /** Starts waiting downloads while fewer than two run. */
    private fun next() {
        for (d in all()) {
            if (d.state != "waiting") continue
            if (running.size >= DownloadRules.AT_ONCE) break
            run(d)
        }
    }

    private fun run(d: Download) {
        val job = begin(d, { size, received -> progress(d.id, received, size) }, { saved, size, received, why -> ended(d.id, saved, received, size, why) })
        if (job == null) {
            runCatching { store.updateDownload(d.id, d.size, d.received, "failed") }
            return
        }
        running[d.id] = job
        runCatching { store.updateDownload(d.id, d.size, d.received, "loading") }
    }

    /** Pauses a running download, or carries on with a paused or failed one. */
    fun toggle(id: Long) {
        running[id]?.let {
            pausing += id
            it.cancel()
            return
        }
        val d = all().firstOrNull { it.id == id } ?: return
        if (d.state == "done") return
        runCatching { store.updateDownload(id, d.size, d.received, "waiting") }
        next()
        show()
    }

    /** Stops it, deletes its file and forgets it. */
    fun remove(id: Long) {
        running.remove(id)?.let {
            pausing += id
            it.cancel()
        }
        live.remove(id)
        val d = all().firstOrNull { it.id == id } ?: return
        val file = File(d.path)
        // Only ever delete inside Haven's downloads folder.
        if (file.canonicalPath.startsWith(root.canonicalPath + File.separator)) {
            file.delete()
            partOf(file).delete()
        }
        runCatching { store.removeDownload(id) }
        pictureFile(id).delete()
        next()
        show()
    }

    /** A saved download as something to play. */
    fun playRequest(d: Download): PlayRequest? {
        if (d.state != "done" || !File(d.path).exists()) return null
        return PlayRequest(d.path, d.title, d.detail, d.kind, d.titleId, d.videoId, d.poster, "Downloaded")
    }

    fun playRequest(id: Long): PlayRequest? = all().firstOrNull { it.id == id }?.let(::playRequest)

    // News from a running download

    fun progress(id: Long, received: Long, size: Long) {
        val whole = if (size > 0) size else live[id]?.first ?: 0
        live[id] = whole to received
        runCatching { store.updateDownload(id, whole, received, "loading") }
        show()
    }

    /** One download ended: saved, paused, or failed. */
    fun ended(id: Long, saved: Boolean, received: Long, size: Long, why: String?) {
        running.remove(id)
        live.remove(id)
        val paused = pausing.remove(id)
        val state = if (saved) "done" else if (paused) "paused" else "failed"
        if (why != null && !saved && !paused) Log.note("download failed: $why")
        // A removed download has no row to update.
        runCatching { store.updateDownload(id, maxOf(size, received), received, state) }
        if (saved) all().firstOrNull { it.id == id }?.let { saved(File(it.path)) }
        next()
        show()
    }

    /** Fills the Library's list. */
    fun show() {
        val downloads = all()
        rows = DownloadRules.rows(downloads, live) { id -> pictureFile(id).takeIf { it.exists() } }
        count = downloads.size
        revision += 1
    }

    private fun partOf(file: File) = File(file.parentFile, file.name + ".part")

    /** Fetches into `<file>.part`, carrying on from what is there, then
     *  renames it. */
    private suspend fun fetch(d: Download, progress: (Long, Long) -> Unit, ended: (Boolean, Long, Long, String?) -> Unit) {
        val file = File(d.path)
        val part = partOf(file)
        var size = d.size
        var have = 0L
        val result = runCatching {
            withContext(Dispatchers.IO) {
                file.parentFile?.mkdirs()
                have = if (part.exists()) part.length() else 0
                val headers = if (have > 0) listOf("Range" to "bytes=$have-") else emptyList()
                http.open(d.url, headers = headers, readTimeoutSeconds = 120).use { response ->
                    if (!response.isSuccessful) error("the server answered ${response.code}")
                    // A server that ignores the range sends it all again from the start.
                    val resumed = response.code == 206
                    if (!resumed) have = 0
                    val length = response.body?.contentLength() ?: -1
                    size = if (length >= 0) length + have else d.size
                    val body = response.body ?: error("the answer was empty")
                    FileOutputStream(part, resumed).use { out ->
                        val buffer = ByteArray(256 * 1024)
                        val input = body.byteStream()
                        var reported = System.currentTimeMillis()
                        while (isActive) {
                            val read = input.read(buffer)
                            if (read < 0) break
                            out.write(buffer, 0, read)
                            have += read
                            if (System.currentTimeMillis() - reported >= DownloadRules.REPORT_EVERY_MILLIS) {
                                reported = System.currentTimeMillis()
                                val (s, h) = size to have
                                withContext(main) { progress(s, h) }
                            }
                        }
                    }
                }
            }
        }
        val stopped = !kotlinx.coroutines.currentCoroutineContext().isActive
        withContext(kotlinx.coroutines.NonCancellable + main) {
            when {
                stopped -> ended(false, size, have, null)
                result.isFailure -> ended(false, size, have, Http.words(result.exceptionOrNull()!!))
                size > 0 && have < size -> ended(false, size, have, "the connection ended early")
                else -> {
                    val moved = withContext(Dispatchers.IO) { part.renameTo(file) }
                    if (moved) ended(true, maxOf(size, have), have, null) else ended(false, size, have, "the file could not be saved")
                }
            }
        }
    }
}
