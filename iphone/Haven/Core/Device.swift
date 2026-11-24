import Foundation

/// Identifies this copy of Haven to media servers (kv `device_id`, made
/// once and kept, `haven-` and 32 hex digits). An update from the Rust
/// build keeps the same one, so servers see the same device.
enum DeviceID {
    static let key = "device_id"

    static func get(_ store: Store) -> String {
        if let id = try? store.get(key, as: String.self), !id.isEmpty {
            return id
        }
        let id = "haven-" + (0 ..< 16).map { _ in String(format: "%02x", UInt8.random(in: 0 ... 255)) }.joined()
        do {
            try store.put(key, id)
        } catch {
            Log.note("could not save the device id: \(error.localizedDescription)")
        }
        return id
    }
}

/// Runs `work`, giving up after `seconds`: nil then. Used where the Rust
/// build waited with a time limit (a server counts as offline after 8 s).
func withTimeout<T: Sendable>(_ seconds: Double, _ work: @escaping @Sendable () async throws -> T) async rethrows -> T? {
    try await withThrowingTaskGroup(of: T?.self) { group in
        group.addTask { try await work() }
        group.addTask {
            try? await Task.sleep(for: .seconds(seconds))
            return nil
        }
        let first = try await group.next() ?? nil
        group.cancelAll()
        return first
    }
}
