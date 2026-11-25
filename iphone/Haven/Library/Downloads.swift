import Foundation
import Observation

/// Films and episodes saved to this iPhone, in Files › On My iPhone ›
/// Haven › Downloads, and the Library's list of them.
///
/// Unlike the Rust build, downloads go on with Haven in the background or
/// closed (user, 2026-11-25): Apple's background download service fetches
/// them, and wakes Haven when they are done. Two run at once; the rest wait.
/// A paused download keeps what it has (`<file>.resume`) and carries on
/// from there.
@MainActor @Observable
final class Downloads {
    private(set) var rows: [DownloadRow] = []
    private(set) var count = 0
    /// Goes up whenever a download changes (the title page's buttons read it).
    private(set) var revision = 0

    @ObservationIgnored private let store: Store
    @ObservationIgnored let root: URL
    @ObservationIgnored private let session: DownloadSession
    /// Running downloads, by number.
    @ObservationIgnored private var running: [Int64: URLSessionDownloadTask] = [:]
    /// Bytes so far of each running download (newer than the database).
    @ObservationIgnored private var live: [Int64: (size: Int64, received: Int64)] = [:]
    /// Being paused (their end is not a failure).
    @ObservationIgnored private var pausing: Set<Int64> = []
    @ObservationIgnored private var started = false
    /// Starts fetching one download into a file (Apple's background
    /// service; a stand-in in tests).
    @ObservationIgnored var begin: (_ id: Int64, _ url: URL, _ path: URL) -> URLSessionDownloadTask?
    @ObservationIgnored var pauseTask: (URLSessionDownloadTask) -> Void

    init(store: Store, root: URL = DownloadRules.downloadsFolder, session: DownloadSession = .shared) {
        self.store = store
        self.root = root
        self.session = session
        begin = { session.start(id: $0, url: $1, path: $2) }
        pauseTask = { session.pause($0) }
    }

    /// Picks up what the background service is still fetching, carries on
    /// with what waits, and lists them all.
    func start() {
        guard !started else { return }
        started = true
        // The folder shows in the Files app only once it exists.
        try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        session.owner = self
        session.tasks { [weak self] tasks in
            guard let self else { return }
            for task in tasks {
                guard let id = DownloadSession.id(of: task) else { continue }
                if task.state == .running || task.state == .suspended { running[id] = task }
            }
            // A download marked running with no task left (Haven was ended
            // meanwhile) waits its turn again.
            for d in all() where (d.state == "loading" && running[d.id] == nil) {
                try? store.updateDownload(id: d.id, size: d.size, received: d.received, state: "waiting")
            }
            next()
            show()
        }
    }

    private func all() -> [Download] { (try? store.downloads()) ?? [] }

    // MARK: From the title page

    /// Whether the stream at `address` was downloaded: 0 no, 1 on its way, 2 saved.
    func savedState(_ address: String) -> Int {
        _ = revision
        guard let d = all().first(where: { $0.url == address && $0.state != "failed" }) else { return 0 }
        return d.state == "done" ? 2 : 1
    }

    /// What pressing a row's download button does. Says what happened, in
    /// words for the title page; `play` plays a saved copy.
    func download(_ request: PlayRequest, season: Int?, episode: Int?, play: (PlayRequest) -> Void) -> String {
        let existing = all()
        // This one is downloaded already: play the saved copy (or say how far it has come).
        if let saved = existing.first(where: { $0.url == request.address && $0.state != "failed" }) {
            if saved.state == "done" {
                if let copy = playRequest(saved) { play(copy) }
                return ""
            }
            let (size, received) = live[saved.id] ?? (saved.size, saved.received)
            guard size > 0 else { return "Still downloading. Press play to stream it now." }
            let share = Int((Double(received) / Double(size) * 100).rounded())
            return "Still downloading (\(share)%). Press play to stream it now."
        }
        var d = Download(kind: season != nil ? "series" : request.kind, titleID: request.titleID, videoID: request.videoID,
                         title: request.title, detail: request.detail, season: season, episode: episode,
                         poster: request.poster, url: request.address, path: "", size: 0, received: 0,
                         state: "waiting", addedAt: nowMillis())
        // The same film or episode already saved or on its way.
        if existing.contains(where: { $0.kind == d.kind && $0.videoID == d.videoID && $0.state != "failed" }) {
            return "That one is already in your downloads (Library)."
        }
        d.path = unusedPath(DownloadRules.fileName(d, extension: DownloadRules.extensionOf(d.url)), taken: existing).path
        do {
            try store.addDownload(d)
        } catch {
            Log.note("could not add a download: \(error.localizedDescription)")
            return "Could not start the download."
        }
        next()
        show()
        return "Downloading to \(DownloadRules.placeWords). It is in your Library under Downloads."
    }

    /// A file name not used by another download (two sources of one title).
    private func unusedPath(_ name: String, taken: [Download]) -> URL {
        let used = Set(taken.map { DownloadRules.currentPath($0.path, root: root).lastPathComponent })
        var candidate = name
        var number = 2
        let stem = (name as NSString).deletingPathExtension
        let ext = (name as NSString).pathExtension
        while used.contains(candidate) || FileManager.default.fileExists(atPath: root.appendingPathComponent(candidate).path) {
            candidate = "\(stem) (\(number)).\(ext)"
            number += 1
        }
        return root.appendingPathComponent(candidate)
    }

    // MARK: Running

    /// Starts waiting downloads while fewer than two run.
    private func next() {
        for d in all() where d.state == "waiting" {
            guard running.count < DownloadRules.atOnce else { break }
            run(d)
        }
    }

    private func run(_ d: Download) {
        let path = DownloadRules.currentPath(d.path, root: root)
        guard let url = URL(string: d.url), let task = begin(d.id, url, path) else {
            try? store.updateDownload(id: d.id, size: d.size, received: d.received, state: "failed")
            return
        }
        running[d.id] = task
        try? store.updateDownload(id: d.id, size: d.size, received: d.received, state: "loading")
    }

    /// Pauses a running download, or carries on with a paused or failed one.
    func toggle(_ id: Int64) {
        if let task = running[id] {
            pausing.insert(id)
            pauseTask(task)
            return
        }
        guard let d = all().first(where: { $0.id == id }), d.state != "done" else { return }
        try? store.updateDownload(id: id, size: d.size, received: d.received, state: "waiting")
        next()
        show()
    }

    /// Stops it, deletes its file and forgets it.
    func remove(_ id: Int64) {
        if let task = running.removeValue(forKey: id) {
            pausing.insert(id)
            task.cancel()
        }
        live[id] = nil
        guard let d = all().first(where: { $0.id == id }) else { return }
        let path = DownloadRules.currentPath(d.path, root: root)
        // Only ever delete inside Haven's Downloads folder.
        if path.standardizedFileURL.path.hasPrefix(root.standardizedFileURL.path) {
            try? FileManager.default.removeItem(at: path)
            try? FileManager.default.removeItem(at: DownloadSession.resumeFile(path))
        }
        try? store.removeDownload(id: id)
        next()
        show()
    }

    /// A saved download as something to play.
    func playRequest(_ d: Download) -> PlayRequest? {
        let path = DownloadRules.currentPath(d.path, root: root)
        guard d.state == "done", FileManager.default.fileExists(atPath: path.path) else { return nil }
        return PlayRequest(address: path.path, title: d.title, detail: d.detail, kind: d.kind, titleID: d.titleID,
                           videoID: d.videoID, poster: d.poster, source: "Downloaded")
    }

    func playRequest(id: Int64) -> PlayRequest? {
        all().first { $0.id == id }.flatMap(playRequest)
    }

    // MARK: From the background service

    func progress(_ id: Int64, received: Int64, size: Int64) {
        let size = size > 0 ? size : live[id]?.size ?? 0
        live[id] = (size, received)
        try? store.updateDownload(id: id, size: size, received: received, state: "loading")
        show()
    }

    /// One download ended: saved, paused, or failed.
    func ended(_ id: Int64, saved: Bool, received: Int64, size: Int64, why: String?) {
        running[id] = nil
        live[id] = nil
        let paused = pausing.remove(id) != nil
        let state = saved ? "done" : paused ? "paused" : "failed"
        if let why, !saved, !paused { Log.note("download failed: \(why)") }
        // A removed download has no row to update.
        try? store.updateDownload(id: id, size: max(size, received), received: received, state: state)
        next()
        show()
    }

    /// Fills the Library's list.
    func show() {
        let downloads = all()
        rows = DownloadRules.rows(downloads, live: live)
        count = downloads.count
        revision += 1
    }
}

/// Apple's background download service for Haven: one session, the same
/// name every run, so downloads started before Haven was closed are found
/// again. Its callbacks come on its own queue; the file is moved into place
/// there (it is gone once the callback returns), the rest is passed to
/// `Downloads` on the main thread.
final class DownloadSession: NSObject, URLSessionDownloadDelegate, @unchecked Sendable {
    static let identifier = "net.haven.app.downloads"
    static let shared = DownloadSession()

    /// Told about progress and ends; set once the Library is up. What comes
    /// before is held and passed on then.
    @MainActor weak var owner: Downloads? {
        didSet { if let owner { held.forEach { $0(owner) }; held.removeAll() } }
    }
    @MainActor private var held: [(Downloads) -> Void] = []
    /// Given by iOS when it woke Haven for finished downloads; called once
    /// their news is handled.
    @MainActor var finishedEvents: (() -> Void)?

    private let lock = NSLock()
    private var reported: [Int: Date] = [:]

    private lazy var session: URLSession = {
        let configuration = URLSessionConfiguration.background(withIdentifier: Self.identifier)
        configuration.sessionSendsLaunchEvents = true
        configuration.isDiscretionary = false
        configuration.timeoutIntervalForResource = 60 * 60 * 24
        return URLSession(configuration: configuration, delegate: self, delegateQueue: nil)
    }()

    /// Where a paused download keeps what it has.
    static func resumeFile(_ path: URL) -> URL {
        path.deletingLastPathComponent().appendingPathComponent("." + path.lastPathComponent + ".resume")
    }

    /// The download's number and file, carried by its task.
    static func id(of task: URLSessionTask) -> Int64? {
        task.taskDescription?.split(separator: "\n", maxSplits: 1).first.flatMap { Int64($0) }
    }

    private static func path(of task: URLSessionTask) -> URL? {
        guard let parts = task.taskDescription?.split(separator: "\n", maxSplits: 1), parts.count == 2 else { return nil }
        return URL(fileURLWithPath: String(parts[1]))
    }

    @MainActor
    func tasks(_ found: @escaping @MainActor ([URLSessionDownloadTask]) -> Void) {
        session.getTasksWithCompletionHandler { _, _, downloads in
            let tasks = downloads
            Task { @MainActor in found(tasks) }
        }
    }

    /// Starts (or carries on with) a download into `path`.
    @MainActor
    func start(id: Int64, url: URL, path: URL) -> URLSessionDownloadTask? {
        let resume = Self.resumeFile(path)
        let task: URLSessionDownloadTask
        if let data = try? Data(contentsOf: resume) {
            task = session.downloadTask(withResumeData: data)
            try? FileManager.default.removeItem(at: resume)
        } else {
            task = session.downloadTask(with: url)
        }
        task.taskDescription = "\(id)\n\(path.path)"
        task.resume()
        return task
    }

    /// Pauses: what has arrived is kept for carrying on.
    @MainActor
    func pause(_ task: URLSessionDownloadTask) {
        let path = Self.path(of: task)
        task.cancel { data in
            if let data, let path { try? data.write(to: Self.resumeFile(path)) }
        }
    }

    private func tell(_ news: @escaping @MainActor (Downloads) -> Void) {
        Task { @MainActor in
            if let owner = self.owner { news(owner) } else { self.held.append(news) }
        }
    }

    // MARK: The service's news

    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask, didWriteData bytesWritten: Int64,
                    totalBytesWritten: Int64, totalBytesExpectedToWrite: Int64) {
        guard let id = Self.id(of: downloadTask) else { return }
        let due = lock.withLock {
            let now = Date()
            guard now.timeIntervalSince(reported[downloadTask.taskIdentifier] ?? .distantPast) >= DownloadRules.reportEvery else { return false }
            reported[downloadTask.taskIdentifier] = now
            return true
        }
        guard due else { return }
        let size = max(totalBytesExpectedToWrite, 0)
        tell { $0.progress(id, received: totalBytesWritten, size: size) }
    }

    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask, didFinishDownloadingTo location: URL) {
        guard let id = Self.id(of: downloadTask), let path = Self.path(of: downloadTask) else { return }
        let status = (downloadTask.response as? HTTPURLResponse)?.statusCode ?? 200
        let size = downloadTask.countOfBytesReceived
        guard (200 ..< 300).contains(status) else {
            tell { $0.ended(id, saved: false, received: 0, size: 0, why: "the server answered \(status)") }
            return
        }
        do {
            try FileManager.default.createDirectory(at: path.deletingLastPathComponent(), withIntermediateDirectories: true)
            if FileManager.default.fileExists(atPath: path.path) { try FileManager.default.removeItem(at: path) }
            try FileManager.default.moveItem(at: location, to: path)
            let length = (try? FileManager.default.attributesOfItem(atPath: path.path)[.size] as? Int64) ?? size
            tell { $0.ended(id, saved: true, received: length, size: length, why: nil) }
        } catch {
            let why = error.localizedDescription
            tell { $0.ended(id, saved: false, received: 0, size: 0, why: why) }
        }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: (any Error)?) {
        lock.withLock { _ = reported.removeValue(forKey: task.taskIdentifier) }
        guard let error, let id = Self.id(of: task) else { return }
        // What arrived before it broke off is kept for carrying on.
        if let data = (error as NSError).userInfo[NSURLSessionDownloadTaskResumeData] as? Data, let path = Self.path(of: task) {
            try? data.write(to: Self.resumeFile(path))
        }
        let (received, size) = (task.countOfBytesReceived, task.countOfBytesExpectedToReceive)
        let why = error.localizedDescription
        tell { $0.ended(id, saved: false, received: received, size: max(size, 0), why: why) }
    }

    func urlSessionDidFinishEvents(forBackgroundURLSession session: URLSession) {
        Task { @MainActor in
            self.finishedEvents?()
            self.finishedEvents = nil
        }
    }
}
