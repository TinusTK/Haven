import Foundation

/// The folders Haven keeps its files in.
///
/// On iPhone this is `Library/Application Support/Haven` inside the app's
/// container: the same folder the Rust build used, so an update finds the
/// old database, log and secrets file.
struct AppPaths: Sendable {
    let dataDir: URL

    /// The usual place, created if missing.
    static func discover() throws -> AppPaths {
        guard let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else {
            throw HavenError.noHomeFolder
        }
        return try at(support.appendingPathComponent("Haven", isDirectory: true))
    }

    /// A given folder, created if missing (also used by tests).
    static func at(_ folder: URL) throws -> AppPaths {
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        } catch {
            throw HavenError.io(path: folder.path, message: error.localizedDescription)
        }
        return AppPaths(dataDir: folder)
    }

    var database: URL { dataDir.appendingPathComponent("haven.sqlite3") }
    var logFile: URL { dataDir.appendingPathComponent("haven.log") }
    var secretsFile: URL { dataDir.appendingPathComponent("secrets.env") }
}
