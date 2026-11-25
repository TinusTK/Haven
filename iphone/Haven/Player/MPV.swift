import Foundation
import Mpv
import OpenGLES
import QuartzCore
import UIKit

/// mpv, Haven's player, and where it draws.
///
/// The libmpv Haven ships can only draw with OpenGL, so mpv gets an OpenGL
/// ES context of its own and draws each frame into a layer made for it
/// (`MPVView`); SwiftUI's controls sit on top. mpv is created off the main
/// thread and lives until the app ends; each film replaces the last.
/// The Rust build's `haven-player/src/video.rs` (and `video_ios.rs`).
@MainActor
final class MPV {
    /// What mpv reported since the last `poll`.
    enum Event: Equatable {
        /// The file is open and about to play.
        case loaded
        /// Played to the end.
        case finished
        /// Could not be opened, or broke off; mpv's reason in words.
        case failed(String)
    }

    /// Where playback stands right now.
    struct Status: Equatable {
        var position: Double
        /// 0 while unknown and for live streams.
        var duration: Double
        var paused: Bool
        /// Waiting for more of the stream.
        var buffering: Bool
        /// Bytes a second arriving, if known.
        var arriving: Double?
        /// Seconds past the position that have already arrived.
        var ahead: Double?
    }

    /// How the picture fills the screen.
    enum Fit: Int, CaseIterable {
        /// All of it, with bars where the shapes differ.
        case whole = 0
        /// The screen filled, the picture's edges cut off.
        case fill = 1
        /// The screen filled, the picture pulled out of shape.
        case stretch = 2
    }

    /// What mpv knows about what is playing.
    struct Details: Equatable {
        var width: Int?
        var height: Int?
        var framesPerSecond: Double?
        var video: String?
        var audio: String?
        var channels: String?
        var container: String?
        /// Apple's decoder, when it is the one in use.
        var decoder: String?
    }

    static let shared = MPV()

    private(set) var handle: OpaquePointer?
    /// mpv is being created.
    private var starting = false
    /// Why mpv cannot work here, once known.
    private(set) var broken: String?
    /// What to load as soon as mpv is ready.
    private var waiting: (address: String, at: Double?)?
    /// The view mpv draws into: one for the whole run, put on screen while
    /// the player shows (its drawing stops while it is off screen).
    let view = MPVView()

    /// mpv puts what it has read ahead here, on disk, and the folder is
    /// emptied when a film closes.
    static var cacheFolder: URL { FileManager.default.temporaryDirectory.appendingPathComponent("haven-stream", isDirectory: true) }

    // MARK: Starting

    /// Creates mpv off the main thread, with Haven's options.
    private func start() {
        guard handle == nil, !starting, broken == nil else { return }
        starting = true
        let cache = Self.cacheFolder.path
        try? FileManager.default.createDirectory(at: Self.cacheFolder, withIntermediateDirectories: true)
        Task.detached(priority: .userInitiated) {
            let made = Self.create(cache: cache)
            await MainActor.run {
                self.starting = false
                switch made {
                case let .success(pointer):
                    self.handle = pointer.value
                    self.view.attach(self)
                    if let waiting = self.waiting {
                        self.waiting = nil
                        self.load(waiting.address, at: waiting.at)
                    }
                case let .failure(problem):
                    self.broken = problem.words
                    Log.note("the player cannot work here: \(problem.words)")
                }
            }
        }
    }

    /// A pointer that may cross to the main thread (mpv's handle is safe to
    /// use from any thread).
    struct Handle: @unchecked Sendable { let value: OpaquePointer }

    nonisolated private static func create(cache: String) -> Result<Handle, Problem> {
        guard let mpv = mpv_create() else { return .failure(Problem("mpv could not start")) }
        func option(_ name: String, _ value: String) { mpv_set_option_string(mpv, name, value) }
        option("vo", "libmpv")
        // Apple's own decoder (VideoToolbox), straight into OpenGL ES where
        // it can (devices), else copied back (the simulator).
        option("hwdec", "videotoolbox,videotoolbox-copy")
        // Apple's sound decoders first where they cover the format.
        option("ad", "ac3_at,eac3_at,aac_at,mp3_at")
        option("video-timing-offset", "0")
        // Addresses are opened as they are: no helper programs.
        option("ytdl", "no")
        option("sub-auto", "no")
        option("audio-display", "no")
        option("network-timeout", "30")
        // Fetch the whole film, not just the next minutes: the read-ahead
        // goes to a file on disk, and what was watched is kept for going back.
        option("cache", "yes")
        option("cache-on-disk", "yes")
        option("demuxer-cache-dir", cache)
        option("demuxer-max-bytes", "200GiB")
        option("demuxer-max-back-bytes", "200GiB")
        option("cache-secs", "1000000")
        option("demuxer-readahead-secs", "1000000")
        // Haven keeps no settings of mpv's own and loads no scripts.
        option("config", "no")
        option("load-scripts", "no")
        option("input-default-bindings", "no")
        #if DEBUG
        option("terminal", "yes")
        option("msg-level", "all=warn")
        #endif
        let done = mpv_initialize(mpv)
        guard done >= 0 else {
            mpv_terminate_destroy(mpv)
            return .failure(Problem("mpv could not start (\(String(cString: mpv_error_string(done))))"))
        }
        return .success(Handle(value: mpv))
    }

    // MARK: Playing

    /// Plays a web address (or a file here), from `at` seconds in or from
    /// the start. Whatever was playing is replaced.
    func play(_ address: String, at: Double?) {
        guard broken == nil else { return }
        if handle == nil || !view.ready {
            waiting = (address, at)
            start()
            return
        }
        load(address, at: at)
    }

    /// The view can draw now: loads what was waiting for it.
    func viewReady() {
        guard handle != nil, let waiting else { return }
        self.waiting = nil
        load(waiting.address, at: waiting.at)
    }

    private func load(_ address: String, at: Double?) {
        guard let handle else { return }
        // Only once mpv can draw: a file loaded before has nowhere to draw
        // and plays without its picture.
        guard view.ready else {
            waiting = (address, at)
            return
        }
        set("start", at.map { String(format: "%.1f", $0) } ?? "none")
        set("pause", "no")
        command(handle, ["loadfile", address, "replace"])
    }

    /// Stops playing and empties the read-ahead; mpv stays ready.
    func stop() {
        waiting = nil
        if let handle { command(handle, ["stop"]) }
        let cache = Self.cacheFolder
        Task.detached {
            let names = (try? FileManager.default.contentsOfDirectory(atPath: cache.path)) ?? []
            for name in names { try? FileManager.default.removeItem(at: cache.appendingPathComponent(name)) }
        }
    }

    /// Empties mpv's event queue and returns what matters from it.
    func poll() -> [Event] {
        guard let handle else { return [] }
        var events: [Event] = []
        while let event = mpv_wait_event(handle, 0)?.pointee, event.event_id != MPV_EVENT_NONE {
            switch event.event_id {
            case MPV_EVENT_FILE_LOADED:
                events.append(.loaded)
            case MPV_EVENT_END_FILE:
                guard let data = event.data?.assumingMemoryBound(to: mpv_event_end_file.self).pointee else { continue }
                if data.reason == MPV_END_FILE_REASON_EOF {
                    events.append(.finished)
                } else if data.reason == MPV_END_FILE_REASON_ERROR {
                    events.append(.failed(Self.reason(data.error)))
                }
            default:
                break
            }
        }
        return events
    }

    /// mpv's reason for giving up on a file, in words.
    nonisolated static func reason(_ code: Int32) -> String {
        switch code {
        case MPV_ERROR_LOADING_FAILED.rawValue: "the address could not be opened"
        case MPV_ERROR_UNKNOWN_FORMAT.rawValue: "the format was not recognised"
        case MPV_ERROR_NOTHING_TO_PLAY.rawValue, MPV_ERROR_UNSUPPORTED.rawValue: "there was no picture or sound in it"
        default: "mpv error \(code)"
        }
    }

    /// Nil until mpv is running.
    var status: Status? {
        guard handle != nil else { return nil }
        return Status(position: double("time-pos") ?? 0, duration: double("duration") ?? 0,
                      paused: flag("pause") ?? false, buffering: flag("paused-for-cache") ?? false,
                      arriving: whole("cache-speed").map { Double(max($0, 0)) },
                      ahead: double("demuxer-cache-duration").map { max($0, 0) })
    }

    func setPaused(_ paused: Bool) { set("pause", paused ? "yes" : "no") }

    func seek(to seconds: Double) {
        if let handle { command(handle, ["seek", String(format: "%.2f", max(seconds, 0)), "absolute"]) }
    }

    func seek(by seconds: Double) {
        if let handle { command(handle, ["seek", String(format: "%.2f", seconds), "relative"]) }
    }

    /// mpv's track list as it gave it (JSON); empty when nothing is loaded.
    var tracksJSON: String { text("track-list") ?? "" }

    func selectAudio(_ id: Int) { set("aid", String(id)) }

    /// Nil turns subtitles off.
    func selectSubtitle(_ id: Int?) { set("sid", id.map(String.init) ?? "no") }

    /// How far subtitles sit above the bottom (the picture is 720 high).
    func setSubtitleMargin(_ margin: Int) { set("sub-margin-y", String(min(max(margin, 0), 400))) }

    /// 0 silent to 1 as recorded.
    func setVolume(_ volume: Double) { set("volume", String(Int((min(max(volume, 0), 1) * 100).rounded()))) }

    func setMuted(_ muted: Bool) { set("mute", muted ? "yes" : "no") }

    /// Dolby and DTS sound sent on as it is, for a receiver to decode.
    func setPassthrough(_ on: Bool) { set("audio-spdif", on ? "ac3,eac3,dts,dts-hd,truehd" : "") }

    /// 1 is normal speed; the pitch is kept.
    func setSpeed(_ speed: Double) { set("speed", String(min(max(speed, 0.25), 4))) }

    func setFit(_ fit: Fit) {
        set("keepaspect", fit == .stretch ? "no" : "yes")
        set("panscan", fit == .fill ? "1.0" : "0.0")
    }

    /// 1 is the usual size.
    func setSubtitleScale(_ scale: Double) { set("sub-scale", String(min(max(scale, 0.3), 3))) }

    /// Shows subtitles this many seconds later (earlier if negative).
    func setSubtitleDelay(_ seconds: Double) { set("sub-delay", String(min(max(seconds, -60), 60))) }

    /// Moves the subtitles so the next line (1) or the one before (-1)
    /// shows now. Returns the delay that results.
    func syncSubtitles(_ way: Int) -> Double? {
        guard let handle else { return nil }
        command(handle, ["sub-step", way < 0 ? "-1" : "1"])
        return double("sub-delay")
    }

    var details: Details {
        func trimmed(_ name: String) -> String? {
            text(name).map { $0.trimmingCharacters(in: .whitespaces) }.flatMap { $0.isEmpty ? nil : $0 }
        }
        return Details(width: whole("width").flatMap { $0 > 0 ? Int($0) : nil },
                       height: whole("height").flatMap { $0 > 0 ? Int($0) : nil },
                       framesPerSecond: double("container-fps").flatMap { $0 > 0 ? $0 : nil },
                       video: trimmed("video-format") ?? trimmed("video-codec"),
                       audio: trimmed("audio-codec-name") ?? trimmed("audio-codec"),
                       channels: trimmed("audio-params/hr-channels") ?? trimmed("audio-params/channels"),
                       container: trimmed("file-format"),
                       decoder: trimmed("hwdec-current").flatMap { $0 == "no" ? nil : $0 })
    }

    // MARK: mpv's own calls

    private func set(_ name: String, _ value: String) {
        guard let handle else { return }
        mpv_set_property_string(handle, name, value)
    }

    private func text(_ name: String) -> String? {
        guard let handle, let raw = mpv_get_property_string(handle, name) else { return nil }
        defer { mpv_free(raw) }
        return String(cString: raw)
    }

    private func double(_ name: String) -> Double? {
        guard let handle else { return nil }
        var value = 0.0
        return mpv_get_property(handle, name, MPV_FORMAT_DOUBLE, &value) >= 0 ? value : nil
    }

    private func whole(_ name: String) -> Int64? {
        guard let handle else { return nil }
        var value: Int64 = 0
        return mpv_get_property(handle, name, MPV_FORMAT_INT64, &value) >= 0 ? value : nil
    }

    private func flag(_ name: String) -> Bool? {
        guard let handle else { return nil }
        var value: Int32 = 0
        return mpv_get_property(handle, name, MPV_FORMAT_FLAG, &value) >= 0 ? value != 0 : nil
    }

    /// Runs an mpv command; each argument is passed as it is (never read
    /// by a shell or as an option).
    private func command(_ handle: OpaquePointer, _ words: [String]) {
        var pointers: [UnsafeMutablePointer<CChar>?] = words.map { strdup($0) }
        pointers.append(nil)
        defer { pointers.forEach { free($0) } }
        pointers.withUnsafeMutableBufferPointer { buffer in
            buffer.baseAddress!.withMemoryRebound(to: UnsafePointer<CChar>?.self, capacity: buffer.count) {
                _ = mpv_command(handle, $0)
            }
        }
    }
}

/// Finds an OpenGL ES function for mpv (Haven links OpenGLES.framework).
private func glLookup(_: UnsafeMutableRawPointer?, _ name: UnsafePointer<CChar>?) -> UnsafeMutableRawPointer? {
    guard let name else { return nil }
    return dlsym(UnsafeMutableRawPointer(bitPattern: -2), name)
}

/// The layer mpv draws into, behind the player's controls. Frames are drawn
/// on the screen's beat while mpv says a new one is ready.
final class MPVView: UIView {
    override class var layerClass: AnyClass { CAEAGLLayer.self }

    private let context = EAGLContext(api: .openGLES3) ?? EAGLContext(api: .openGLES2)
    private var render: OpaquePointer?
    private var framebuffer: GLuint = 0
    private var colour: GLuint = 0
    private var size: (width: GLint, height: GLint) = (0, 0)
    private var link: CADisplayLink?
    private weak var mpv: MPV?
    /// mpv's render context is made: files may load.
    private(set) var ready = false

    init() {
        super.init(frame: .zero)
        backgroundColor = .black
        isOpaque = true
        contentScaleFactor = UIScreen.main.scale
        (layer as? CAEAGLLayer)?.drawableProperties = [kEAGLDrawablePropertyRetainedBacking: false,
                                                       kEAGLDrawablePropertyColorFormat: kEAGLColorFormatRGBA8]
    }

    required init?(coder: NSCoder) { fatalError("not from a storyboard") }

    /// Hooks mpv's renderer to this view once both exist.
    @MainActor
    func attach(_ mpv: MPV) {
        self.mpv = mpv
        guard render == nil, let handle = mpv.handle, let context, EAGLContext.setCurrent(context) else { return }
        var api = Array("opengl".utf8CString)
        var initParams = mpv_opengl_init_params(get_proc_address: glLookup, get_proc_address_ctx: nil)
        var made: OpaquePointer?
        let result = api.withUnsafeMutableBufferPointer { apiBuffer in
            withUnsafeMutablePointer(to: &initParams) { initPointer in
                var params = [
                    mpv_render_param(type: MPV_RENDER_PARAM_API_TYPE, data: UnsafeMutableRawPointer(apiBuffer.baseAddress)),
                    mpv_render_param(type: MPV_RENDER_PARAM_OPENGL_INIT_PARAMS, data: UnsafeMutableRawPointer(initPointer)),
                    mpv_render_param(type: MPV_RENDER_PARAM_INVALID, data: nil),
                ]
                return mpv_render_context_create(&made, handle, &params)
            }
        }
        guard result >= 0, let made else {
            Log.note("the player cannot draw: \(String(cString: mpv_error_string(result)))")
            return
        }
        render = made
        ready = true
        makeFramebuffer()
        startDrawing()
        mpv.viewReady()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        if render != nil { makeFramebuffer() }
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window == nil {
            link?.invalidate()
            link = nil
        } else if render != nil {
            startDrawing()
        }
    }

    private func startDrawing() {
        guard link == nil else { return }
        let link = CADisplayLink(target: self, selector: #selector(step))
        link.add(to: .main, forMode: .common)
        self.link = link
    }

    /// The layer's own picture, sized to the view.
    private func makeFramebuffer() {
        guard let context, EAGLContext.setCurrent(context), let layer = layer as? CAEAGLLayer else { return }
        if framebuffer != 0 { glDeleteFramebuffers(1, &framebuffer) }
        if colour != 0 { glDeleteRenderbuffers(1, &colour) }
        glGenFramebuffers(1, &framebuffer)
        glGenRenderbuffers(1, &colour)
        glBindFramebuffer(GLenum(GL_FRAMEBUFFER), framebuffer)
        glBindRenderbuffer(GLenum(GL_RENDERBUFFER), colour)
        context.renderbufferStorage(Int(GL_RENDERBUFFER), from: layer)
        glFramebufferRenderbuffer(GLenum(GL_FRAMEBUFFER), GLenum(GL_COLOR_ATTACHMENT0), GLenum(GL_RENDERBUFFER), colour)
        var width: GLint = 0
        var height: GLint = 0
        glGetRenderbufferParameteriv(GLenum(GL_RENDERBUFFER), GLenum(GL_RENDERBUFFER_WIDTH), &width)
        glGetRenderbufferParameteriv(GLenum(GL_RENDERBUFFER), GLenum(GL_RENDERBUFFER_HEIGHT), &height)
        size = (width, height)
        draw(force: true)
    }

    @objc private func step() {
        draw(force: false)
    }

    /// Draws mpv's current frame, when there is a new one (or `force`).
    private func draw(force: Bool) {
        // Drawing with OpenGL while Haven is in the background ends the app.
        guard let render, let context, UIApplication.shared.applicationState != .background,
              size.width > 0, size.height > 0 else { return }
        let wanted = mpv_render_context_update(render) & UInt64(MPV_RENDER_UPDATE_FRAME.rawValue)
        guard wanted != 0 || force, EAGLContext.setCurrent(context) else { return }
        var fbo = mpv_opengl_fbo(fbo: Int32(framebuffer), w: size.width, h: size.height, internal_format: 0)
        var flip: Int32 = 1
        withUnsafeMutablePointer(to: &fbo) { fboPointer in
            withUnsafeMutablePointer(to: &flip) { flipPointer in
                var params = [
                    mpv_render_param(type: MPV_RENDER_PARAM_OPENGL_FBO, data: UnsafeMutableRawPointer(fboPointer)),
                    mpv_render_param(type: MPV_RENDER_PARAM_FLIP_Y, data: UnsafeMutableRawPointer(flipPointer)),
                    mpv_render_param(type: MPV_RENDER_PARAM_INVALID, data: nil),
                ]
                _ = mpv_render_context_render(render, &params)
            }
        }
        glBindRenderbuffer(GLenum(GL_RENDERBUFFER), colour)
        context.presentRenderbuffer(Int(GL_RENDERBUFFER))
    }
}
