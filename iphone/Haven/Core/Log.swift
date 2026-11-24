import Foundation

/// A small log file beside the database (`haven.log`), for finding out
/// afterwards why something did not load. Notes must never hold passwords,
/// tokens or keys: log an address through `plainAddress`.
enum Log {
    /// The log starts afresh once it has grown past this.
    static let maxBytes = 512 * 1024

    private final class File: @unchecked Sendable {
        let lock = NSLock()
        var handle: FileHandle?
    }

    private static let file = File()

    /// Opens the log file. Until this is called, notes only go to the console.
    static func start(_ url: URL) {
        let size = (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? Int) ?? 0
        if size > maxBytes || !FileManager.default.fileExists(atPath: url.path) {
            FileManager.default.createFile(atPath: url.path, contents: nil)
        }
        guard let handle = try? FileHandle(forWritingTo: url) else { return }
        _ = try? handle.seekToEnd()
        file.lock.lock()
        file.handle = handle
        file.lock.unlock()
        note("Haven \(Build.version) started")
    }

    /// Writes one line to the console and to the log file.
    static func note(_ text: String) {
        print("haven: \(text)")
        let seconds = Int(Date().timeIntervalSince1970)
        file.lock.lock()
        defer { file.lock.unlock() }
        try? file.handle?.write(contentsOf: Data("\(seconds) \(text)\n".utf8))
    }

    /// An address without what follows `?` or `#` (where tokens and keys
    /// travel) and without `user:password@`.
    static func plainAddress(_ address: String) -> String {
        let cut = address.split(omittingEmptySubsequences: false, whereSeparator: { $0 == "?" || $0 == "#" })
        let address = cut.first.map(String.init) ?? address
        guard let schemeEnd = address.range(of: "://") else { return address }
        let scheme = address[..<schemeEnd.lowerBound]
        let rest = address[schemeEnd.upperBound...]
        let host: Substring
        let path: Substring
        if let slash = rest.firstIndex(of: "/") {
            host = rest[..<slash]
            path = rest[rest.index(after: slash)...]
        } else {
            host = rest
            path = ""
        }
        let bareHost = host.split(separator: "@", omittingEmptySubsequences: false).last ?? host
        return path.isEmpty ? "\(scheme)://\(bareHost)" : "\(scheme)://\(bareHost)/\(path)"
    }
}
