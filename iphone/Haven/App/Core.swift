import Foundation

/// Haven's foundations, opened once at start: the data folder, the log,
/// the database and the keys to use.
struct Core: Sendable {
    let paths: AppPaths
    let store: Store
    let secrets: Secrets

    static func open() throws -> Core {
        let paths = try AppPaths.discover()
        Log.start(paths.logFile)
        Cache.removeRustCache(paths)
        let store = try Store.open(paths.database)
        let file = try Secrets.fromFile(paths.secretsFile)
        let secrets = try Keys.load(store).resolved(file: file)
        Log.note("database at schema \(try store.schemaVersion()); keys \(secrets)")
        return Core(paths: paths, store: store, secrets: secrets)
    }
}
