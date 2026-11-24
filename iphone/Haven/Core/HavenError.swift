import Foundation

/// What can go wrong in Haven's own files and database.
enum HavenError: Error, Equatable, LocalizedError {
    case noHomeFolder
    case io(path: String, message: String)
    case database(String)
    case json(String)
    case databaseTooNew(found: Int, known: Int)

    var errorDescription: String? {
        switch self {
        case .noHomeFolder:
            "could not find a home folder to keep Haven's data in"
        case let .io(path, message):
            "could not use \(path): \(message)"
        case let .database(message):
            "database error: \(message)"
        case let .json(message):
            "stored data could not be read: \(message)"
        case let .databaseTooNew(found, known):
            "this database was made by a newer Haven (schema \(found), this build knows \(known))"
        }
    }
}

/// Milliseconds since 1970, as Haven stores times.
func nowMillis() -> Int64 {
    Int64((Date().timeIntervalSince1970 * 1000).rounded(.down))
}

/// A failure already put in words for the person using Haven.
struct Problem: Error, Equatable, Sendable, LocalizedError {
    var words: String

    init(_ words: String) {
        self.words = words
    }

    var errorDescription: String? { words }
}
