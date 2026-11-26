package net.haven.app.tracking

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import net.haven.app.core.Change
import net.haven.app.core.Http
import net.haven.app.core.Log
import net.haven.app.core.Secrets
import net.haven.app.core.Store
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

/**
 * Trakt and Simkl in the app: the linked accounts, linking with a code
 * (Settings › Accounts), and sync (Settings › Sync options).
 *
 * Haven keeps everything on this device first and notes each change in a
 * queue in its database. The queue goes to the linked accounts in one go: 20
 * s after a change, 8 s after start, and on "Sync now". A change that cannot
 * be sent stays queued. What the accounts hold is brought in at start, on
 * coming back to Haven (10 minutes or more after the last time), a little
 * after something was watched, every half hour while Haven is open, and on
 * "Sync now". Both ways: what was added on an account comes in, and what was
 * taken off there is taken off here too ([Sync.Seen]). Nothing runs while
 * Haven is closed.
 *
 * Its state changes on its own coroutines; [changed] is called after each
 * change so the screens can show it.
 */
class Tracker(
    private val store: Store,
    keys: Secrets,
    private val http: Http = Http.shared,
    private val scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.Default.limitedParallelism(1)),
) {
    /** How long after a change the queue is sent, after start the first sync
     *  runs, and after a video closes things are brought in (ms). */
    var afterAChangeMillis = 20_000L
    var afterStartMillis = 8_000L
    var afterWatchingMillis = 5_000L

    companion object {
        /** Coming back to Haven brings things in when the last time was this long ago (s). */
        const val PULL_ON_RETURN_SECONDS = 10 * 60L
        /** Brings things in this often while Haven is open (ms). */
        const val PULL_WHILE_OPEN_MILLIS = 30 * 60 * 1000L

        const val SYNC_NOTE = "Changes go out in one go shortly after they happen. " +
            "Haven brings in your accounts' Watchlist and what you watched when it opens, when you come back to it, " +
            "after you watch something, and every half hour while it is open. " +
            "What you take off or mark unwatched on one device goes on the others too. " +
            "Nothing runs while Haven is closed, so it barely touches the battery."
    }

    var keys: Secrets = keys
        private set

    /** Seconds since 1970 (a stand-in clock in tests). */
    var now: () -> Long = { System.currentTimeMillis() / 1000 }
    /** Overrides for the services' addresses (tests). */
    var traktApi = TrackingClient.TRAKT_API
    var simklApi = TrackingClient.SIMKL_API

    @Volatile var accounts: List<TrackedAccount> = TrackedAccount.load(store)
        private set
    @Volatile var running = false
        private set
    @Volatile var lastProblem = ""
        private set
    private var again: Boolean? = null
    private val runLock = Mutex()
    private var pending: Job? = null
    @Volatile private var pendingPull = false
    private var whileOpen: Job? = null

    // Linking (Settings › Accounts).
    @Volatile var linking = false
        private set
    @Volatile var code = ""
        private set
    @Volatile var link = ""
        private set
    @Volatile var linkStatus = ""
        private set
    private var linkJob: Job? = null

    /** Told when a sync brought in Watchlist titles or watched marks. */
    var watchlistChanged: () -> Unit = {}
    var watchedChanged: () -> Unit = {}
    /** After a full pull: catch up what the accounts lack. */
    var pulledAll: () -> Unit = {}
    /** Takes watched marks an account no longer has off this device, each
     *  (kind, IMDb id, video id), with media-server titles linked to them;
     *  says for each whether anything was taken off. The library sets it to
     *  cover linked server titles too. */
    var forgetWatched: (List<Triple<String, String, String>>) -> List<Boolean> = { gone ->
        gone.map { (kind, title, video) ->
            val videos = if (kind == "series") listOf(video) else runCatching { store.finishedVideos(kind, title) }.getOrDefault(emptyList())
            var any = false
            for (v in videos) {
                if (runCatching { store.progress(kind, title, v)?.finished }.getOrNull() == true &&
                    runCatching { store.removeVideoProgress(kind, title, v) }.isSuccess
                ) {
                    any = true
                }
            }
            any
        }
    }
    /** Opens a web page (the service's code page). */
    var openLink: (String) -> Unit = {}
    /** Called after any change the screens show. */
    var changed: () -> Unit = {}

    val client: TrackingClient get() = TrackingClient(keys, http, traktApi, simklApi)

    fun setKeys(keys: Secrets) {
        this.keys = keys
        changed()
    }

    /** Reads the sign-ins again (a restored backup brought one). */
    fun reloadAccounts() {
        accounts = TrackedAccount.load(store)
        changed()
    }

    fun account(service: TrackingService): TrackedAccount? = accounts.firstOrNull { it.service == service }

    // Sync

    val options: Sync.Options get() = Sync.Options.load(store)

    /** The linked accounts whose sync is on. */
    val syncing: List<TrackedAccount>
        get() {
            val off = options.off
            return accounts.filter { it.service.label !in off }
        }

    /** The first run, a little after start, brings things in; so does one every half hour while Haven is open. */
    fun start() {
        schedule(afterStartMillis, pull = true)
        whileOpen?.cancel()
        whileOpen = scope.launch {
            while (isActive) {
                delay(PULL_WHILE_OPEN_MILLIS)
                run(pull = true)
            }
        }
    }

    /** Stops the half-hourly pull (Haven goes to the background). */
    fun stop() {
        whileOpen?.cancel()
        whileOpen = null
    }

    /** Coming back to Haven: brings things in when the last time was a while ago. */
    fun cameBack() {
        if (whileOpen == null) start()
        if (now() - options.lastPull >= PULL_ON_RETURN_SECONDS) schedule(1_000, pull = true)
    }

    /** After a video: the watched mark goes out and what the accounts hold
     *  comes in, so another device's changes show here too. */
    fun watchedSomething() {
        if (syncing.isNotEmpty()) schedule(afterWatchingMillis, pull = true)
    }

    /** Runs the queue later. A run already waiting keeps its pull. */
    @Synchronized
    private fun schedule(afterMillis: Long, pull: Boolean) {
        pendingPull = pendingPull || pull
        pending?.cancel()
        pending = scope.launch {
            delay(afterMillis)
            val wanted = synchronized(this@Tracker) {
                val p = pendingPull
                pendingPull = false
                pending = null
                p
            }
            run(wanted)
        }
    }

    /** Notes a change to send to the linked accounts, and sends the queue a
     *  little later. Nothing linked, or a title the services cannot know (they
     *  go by IMDb id): nothing to send. */
    fun queue(what: String, kind: String, titleId: String, videoId: String) {
        if (syncing.isEmpty() || Tracking.subject(kind, titleId, null) == null) return
        try {
            store.queueChange(Change(what = what, kind = kind, titleId = titleId, videoId = videoId, createdAt = now()))
        } catch (e: Exception) {
            Log.note("could not note a change to sync: ${e.message}")
            return
        }
        changed()
        schedule(afterAChangeMillis, pull = false)
    }

    /** Notes a Watchlist change for the linked accounts. */
    fun trackWatchlist(kind: String, titleId: String, on: Boolean) =
        queue(if (on) "watchlist_add" else "watchlist_remove", kind, titleId, "")

    /** Notes a finished film or episode for the linked accounts. */
    fun trackWatched(kind: String, titleId: String, videoId: String) = queue("watched", kind, titleId, videoId)

    /** Sends the queue, and with `pull` brings in what the accounts hold. A
     *  request while it runs is folded into one more run afterwards. */
    suspend fun run(pull: Boolean) {
        var wanted: Boolean? = pull
        while (wanted != null) {
            val thisPull = wanted
            wanted = null
            if (!runLock.tryLock()) {
                synchronized(this) { again = (again ?: false) || thisPull }
                return
            }
            try {
                val accounts = syncing
                val queued = runCatching { store.queuedChanges() }.getOrDefault(emptyList())
                if (accounts.isEmpty() || (queued.isEmpty() && !thisPull)) return
                running = true
                changed()
                val outcome = Sync.exchange(client, accounts, queued, thisPull, now())
                finished(outcome)
            } finally {
                runLock.unlock()
            }
            synchronized(this) {
                wanted = again
                again = null
            }
        }
    }

    /** "Sync now": sends the queue and brings things in. */
    fun syncNow() {
        scope.launch { run(pull = true) }
    }

    private fun finished(outcome: Sync.Outcome) {
        if (outcome.renewed.isNotEmpty()) {
            accounts = Sync.keepRenewed(accounts, outcome.renewed)
            TrackedAccount.save(accounts, store)
        }
        for (id in outcome.sent) runCatching { store.removeChange(id) }
        var options = options
        if (outcome.problems.isEmpty()) options = options.copy(lastSync = now())
        if (outcome.pulled) {
            val (listChanged, marksChanged) = takeIn(outcome.pulls)
            if (listChanged) watchlistChanged()
            if (marksChanged) watchedChanged()
            if (outcome.problems.isEmpty()) options = options.copy(lastPull = now())
        }
        options.save(store)
        lastProblem = outcome.problems.firstOrNull() ?: ""
        outcome.problems.forEach { Log.note("sync: $it") }
        running = false
        changed()
        if (outcome.pulled && outcome.problems.isEmpty()) pulledAll()
    }

    /** Brings in what changed on each account since its last pull: what was
     *  added there, and what was taken off there goes here too, unless a
     *  change of this device's for it is still waiting to be sent. With more
     *  than one account, what changed on one is passed on to the others (not
     *  on an account's first pull, which would send its whole history). Says
     *  whether the Watchlist and the watched marks changed. */
    private fun takeIn(pulls: List<Sync.Pulled>): Pair<Boolean, Boolean> {
        val seen = (runCatching { store.get<Map<String, Sync.Seen>>(Sync.Seen.KEY) }.getOrNull() ?: emptyMap()).toMutableMap()
        val queued = runCatching { store.queuedChanges() }.getOrDefault(emptyList())
        val others = syncing.size > 1
        var listChanged = false
        var marksChanged = false
        val passOn = mutableListOf<Pair<String, String>>()
        for (pulled in pulls) {
            val label = pulled.service.label
            val first = label !in seen
            val changes = Sync.changesSince(seen[label], pulled)
            val before = passOn.size
            for (title in changes.addedListed) {
                if (Sync.addListed(store, listOf(title), now()) > 0) {
                    listChanged = true
                    passOn += "watchlist_add" to Sync.listedKey(title)
                }
            }
            for (key in changes.goneListed.filter { !Sync.waiting(queued, it) }) {
                val (kind, title, _) = Sync.keyParts(key)
                if (runCatching { store.inWatchlist(kind, title) }.getOrDefault(false) &&
                    runCatching { store.removeFromWatchlist(kind, title) }.isSuccess
                ) {
                    listChanged = true
                    passOn += "watchlist_remove" to key
                }
            }
            for (entry in changes.addedWatched) {
                if (Sync.addWatched(store, listOf(entry)) > 0) {
                    marksChanged = true
                    passOn += "watched" to Sync.watchedKey(entry)
                }
            }
            val gone = changes.goneWatched.filter { !Sync.waiting(queued, it) }
            for ((key, removed) in gone.zip(forgetWatched(gone.map { Sync.keyParts(it) }))) {
                if (removed) {
                    marksChanged = true
                    passOn += "unwatched" to key
                }
            }
            if (first) while (passOn.size > before) passOn.removeAt(passOn.size - 1)
            seen[label] = changes.seen
        }
        runCatching { store.put(Sync.Seen.KEY, seen.toMap()) }.onFailure { Log.note("could not keep what the accounts hold: ${it.message}") }
        if (others) for ((what, key) in passOn) {
            val (kind, title, video) = Sync.keyParts(key)
            queue(what, kind, title, video)
        }
        return listChanged to marksChanged
    }

    /** Switches one account's sync on or off; on, it syncs at once. */
    fun setSync(service: TrackingService, on: Boolean) {
        val options = options
        val off = options.off.filter { it != service.label } + if (on) emptyList() else listOf(service.label)
        options.copy(off = off).save(store)
        changed()
        if (on) scope.launch { run(pull = true) }
    }

    /** What Settings › Sync options shows. */
    data class Status(
        val text: String,
        val waiting: Int,
        val problem: Boolean,
        val busy: Boolean,
        val linked: Boolean,
        val rows: List<Row>,
    ) {
        data class Row(val service: TrackingService, val linked: Boolean, val on: Boolean, val detail: String)
    }

    val status: Status
        get() {
            val options = options
            val syncing = syncing
            val names = syncing.joinToString(" and ") { it.service.label }
            val waiting = if (syncing.isEmpty()) 0 else runCatching { store.queuedChanges().size }.getOrDefault(0)
            val zone = ZoneId.systemDefault()
            val whenText = if (options.lastSync > 0) {
                val at = Instant.ofEpochSecond(options.lastSync).atZone(zone)
                val clock = at.format(DateTimeFormatter.ofPattern("HH:mm", Locale.ENGLISH))
                if (at.toLocalDate() == LocalDate.now(zone)) "Last synced today at $clock"
                else "Last synced ${at.format(DateTimeFormatter.ofPattern("d MMM", Locale.ENGLISH))} at $clock"
            } else {
                "Not synced yet"
            }
            val text = when {
                syncing.isEmpty() -> "Link Trakt or Simkl (or switch one on below) and Haven keeps it up to date from this device."
                running -> "Syncing with $names…"
                lastProblem.isNotEmpty() -> "$whenText. Last try: $lastProblem. Haven tries again later."
                waiting == 0 -> "$whenText. Everything is in step with $names."
                else -> "$whenText. $waiting change${if (waiting == 1) "" else "s"} still to send to $names."
            }
            val rows = TrackingService.entries.map { service ->
                val linked = account(service) != null
                val on = linked && service.label !in options.off
                Status.Row(
                    service, linked, on,
                    if (!linked) "Not linked." else if (on) "Watched, progress and Watchlist, kept in step both ways." else "Linked, but not syncing.",
                )
            }
            return Status(text, waiting, lastProblem.isNotEmpty() && syncing.isNotEmpty(), running, syncing.isNotEmpty(), rows)
        }

    // Linking

    /** What a service's row in Settings › Accounts says. */
    fun rowText(service: TrackingService): String = when {
        account(service) != null -> "Linked: what you finish is marked watched, and your watchlist is passed on."
        !client.hasKeys(service) -> "Not available in this copy of Haven yet (no ${service.label} app key)."
        service == TrackingService.Trakt -> "Not linked. Link opens trakt.tv and shows a short code to approve there."
        else -> "Not linked. Link opens simkl.com: sign in and allow Haven, and it links by itself."
    }

    /** Starts linking: shows the code, opens the service's page and waits for
     *  the user to approve it there. */
    fun connect(service: TrackingService) {
        linkJob?.cancel()
        linking = true
        linkStatus = "Asking ${service.label} for a code…"
        changed()
        val client = client
        linkJob = scope.launch {
            val result = try {
                val signIn = client.begin(service)
                code = signIn.userCode
                link = signIn.link
                linkStatus = if (service == TrackingService.Simkl) {
                    "Sign in on the Simkl page that just opened in your web browser and allow Haven. Haven links by itself."
                } else {
                    "Enter the code on the ${service.label} page that just opened in your web browser."
                }
                changed()
                openLink(signIn.link)
                waitFor(signIn, client)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Result.failure(e)
            }
            connected(service, result)
        }
    }

    /** Asks every `interval` seconds until the code is approved or runs out. */
    private suspend fun waitFor(code: SignInCode, client: TrackingClient): Result<TrackedAccount> {
        val started = System.currentTimeMillis()
        while (true) {
            delay(code.interval * 1000L)
            if (System.currentTimeMillis() - started > code.expiresIn * 1000L) return Result.failure(TrackingError.Expired())
            try {
                client.poll(code, now())?.let { return Result.success(it) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                return Result.failure(e)
            }
        }
    }

    fun cancelLinking() {
        linkJob?.cancel()
        linkJob = null
        idle()
        linkStatus = ""
        changed()
    }

    private fun idle() {
        linking = false
        code = ""
        link = ""
    }

    private fun connected(service: TrackingService, result: Result<TrackedAccount>) {
        idle()
        result.fold(
            { account ->
                accounts = accounts.filter { it.service != service } + account
                TrackedAccount.save(accounts, store)
                linkStatus = "Linked to ${service.label}."
                // Linking turns its sync on: bring in what the new account holds, and send what waits.
                setSync(service, on = true)
            },
            { linkStatus = "Could not link ${service.label}: ${it.message}." },
        )
        changed()
    }

    /** Unlinks: tells the service too (best effort) and forgets the sign-in. */
    fun disconnect(service: TrackingService) {
        account(service)?.let { account ->
            val client = client
            scope.launch { runCatching { client.revoke(account) } }
        }
        accounts = accounts.filter { it.service != service }
        TrackedAccount.save(accounts, store)
        linkStatus = "Unlinked from ${service.label}. Haven has forgotten the sign-in."
        changed()
    }

    /** Copies a service's Watchlist into Haven's. */
    suspend fun importWatchlist(service: TrackingService) {
        val account = account(service) ?: return
        linkStatus = "Fetching your ${service.label} watchlist…"
        changed()
        linkStatus = try {
            val listed = client.watchlist(account)
            val added = Sync.addListed(store, listed, now())
            watchlistChanged()
            "$added of ${listed.size} titles from ${service.label} were new and are now on your Watchlist."
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            "Could not fetch your ${service.label} watchlist: ${e.message}."
        }
        changed()
    }
}
