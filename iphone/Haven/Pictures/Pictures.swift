import Foundation
import ImageIO
import UIKit

/// The sizes Haven shrinks pictures to, in pixels: a picture is fitted
/// inside the box (never made larger).
enum PictureSize: String, Sendable, CaseIterable {
    case poster
    case background
    case logo
    case wide

    var box: (width: Int, height: Int) {
        switch self {
        case .poster: (300, 450)
        case .background: (1600, 900)
        case .logo: (600, 240)
        case .wide: (640, 360)
        }
    }
}

/// Lets a few pieces of work run at once; the rest wait their turn.
actor Gate {
    private let size: Int
    private var running = 0
    private var waiting: [CheckedContinuation<Void, Never>] = []

    init(_ size: Int) {
        self.size = size
    }

    func enter() async {
        if running < size {
            running += 1
            return
        }
        await withCheckedContinuation { waiting.append($0) }
    }

    func leave() {
        if waiting.isEmpty {
            running -= 1
        } else {
            waiting.removeFirst().resume()
        }
    }

    /// Runs `work` in its turn.
    func run<T: Sendable>(_ work: @Sendable () async throws -> T) async rethrows -> T {
        await enter()
        defer { leave() }
        return try await work()
    }
}

/// Holds back work while it is shut (Home's shelves and their pictures wait
/// while Home is not on screen). Looked at again every 2 s, as the Rust
/// build did, and at once when opened.
final class Pause: @unchecked Sendable {
    private let lock = NSLock()
    private var shut = false

    var isShut: Bool {
        lock.lock()
        defer { lock.unlock() }
        return shut
    }

    func set(shut: Bool) {
        lock.lock()
        self.shut = shut
        lock.unlock()
    }

    /// Returns once the pause is open.
    func pass() async {
        while isShut {
            try? await Task.sleep(for: .seconds(2))
            if Task.isCancelled { return }
        }
    }
}

/// Fetches pictures a few at a time, shrinks them, and keeps them on disk:
/// the next time one is wanted it comes from there. Failures are silent
/// (the place for the picture stays empty) and noted in the log without
/// the address's secrets.
/// (`@unchecked`: every stored property is a constant, and NSCache is
/// safe to use from several threads at once.)
final class Pictures: @unchecked Sendable {
    static let shared = Pictures()

    /// The most a picture may take.
    static let maxBytes = 6 * 1024 * 1024
    /// A picture larger than this either way is not read.
    static let maxSide = 8000
    static let atOnce = 6

    let folder: URL
    private let http: HTTP
    private let gate = Gate(Pictures.atOnce)
    /// Pictures shrunk this run, so a picture seen again needs no reading.
    private let memory: NSCache<NSString, UIImage> = {
        let cache = NSCache<NSString, UIImage>()
        cache.totalCostLimit = 64 * 1024 * 1024
        return cache
    }()

    init(folder: URL = Cache.pictures, http: HTTP = .shared) {
        self.folder = folder
        self.http = http
    }

    /// The file a picture is kept in: named after its address (without what
    /// follows `?`, so a new sign-in token does not make it a new picture)
    /// and the size asked for.
    func file(_ url: String, _ size: PictureSize) -> URL {
        folder.appendingPathComponent(Cache.hashName("\(Log.plainAddress(url))|\(size.rawValue)"))
    }

    /// A picture, from memory, the disk, or the network; nil if it cannot be
    /// had. `pause` holds the work back while shut.
    func load(_ url: String, _ size: PictureSize, pause: Pause? = nil) async -> UIImage? {
        let file = file(url, size)
        let key = file.lastPathComponent as NSString
        if let known = memory.object(forKey: key) { return known }
        await pause?.pass()
        do {
            let image = try await gate.run { try await self.fetch(url, size, file: file) }
            memory.setObject(image, forKey: key, cost: Int(image.size.width * image.size.height * image.scale * image.scale * 4))
            return image
        } catch {
            Log.note("picture not shown: \(Log.plainAddress(url)) (\(error.localizedDescription))")
            return nil
        }
    }

    private func fetch(_ url: String, _ size: PictureSize, file: URL) async throws -> UIImage {
        if let data = try? Data(contentsOf: file) {
            if let image = Self.shrink(data, to: size) { return image }
            // A damaged file: fetch the picture again.
            try? FileManager.default.removeItem(at: file)
        }
        let data: Data
        do {
            data = try await http.getBytes(url, limit: Self.maxBytes)
        } catch let error as HTTPError {
            throw Problem(error.localizedDescription)
        }
        guard let image = Self.shrink(data, to: size) else {
            let start = data.prefix(8).map { String(format: "%02x", $0) }.joined(separator: " ")
            throw Problem("not a picture Haven can read; \(data.count) bytes starting \(start)")
        }
        // Kept only once it is known to be a picture, whole or not at all.
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try? data.write(to: file, options: .atomic)
        return image
    }

    /// Reads a picture and shrinks it to fit `size`; nil if it is not one,
    /// or is larger than 8000 pixels either way.
    static func shrink(_ data: Data, to size: PictureSize) -> UIImage? {
        guard let source = CGImageSourceCreateWithData(data as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let width = properties[kCGImagePropertyPixelWidth] as? Int,
              let height = properties[kCGImagePropertyPixelHeight] as? Int,
              width > 0, height > 0, width <= maxSide, height <= maxSide
        else { return nil }
        let (boxWidth, boxHeight) = size.box
        let scale = min(Double(boxWidth) / Double(width), Double(boxHeight) / Double(height), 1)
        let longest = max(1, Int((Double(max(width, height)) * scale).rounded()))
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: longest,
            kCGImageSourceShouldCacheImmediately: true,
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return nil }
        return UIImage(cgImage: image)
    }

    // MARK: Settings › Downloads & storage

    /// How many pictures are kept, and how much room they take.
    func usage() -> (count: Int, bytes: Int64) {
        let files = (try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: [.fileSizeKey])) ?? []
        let bytes = files.reduce(Int64(0)) { $0 + Int64((try? $1.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0) }
        return (files.count, bytes)
    }

    /// The line Settings shows.
    func usageText() -> String {
        let (count, bytes) = usage()
        if count == 0 { return "No pictures are kept on this iPhone yet." }
        return "\(count) picture\(count == 1 ? "" : "s") kept on this iPhone, \(String(format: "%.1f", Double(bytes) / (1024 * 1024))) MB."
    }

    /// Deletes every kept picture and kept answer. Pictures on screen stay;
    /// they are fetched again the next time they are needed.
    func clear(kept: Kept = Cache.kept) {
        kept.clear()
        try? FileManager.default.removeItem(at: folder)
        memory.removeAllObjects()
    }
}
