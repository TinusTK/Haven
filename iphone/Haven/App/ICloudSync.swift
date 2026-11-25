import Foundation
import UIKit

/// Keeps the shared setup (`SharedSetup`: media servers, Live TV and
/// accounts) the same on every device signed in to the same iCloud
/// account, the iPhone/iPad and Apple TV apps alike, through iCloud's
/// key-value storage. It looks when Haven starts or comes back, when
/// iCloud says another device changed it, and every little while for
/// changes made here. Needs the app's iCloud key-value entitlement (the
/// App Store / TestFlight builds); without it nothing is shared.
@MainActor
final class ICloudSync {
    /// The key the shared setup is kept under in iCloud.
    static let cloudKey = "shared_setup"
    /// How often changes made here are looked for while Haven is open.
    static let every: Duration = .seconds(20)

    private let store: Store
    private let cloud = NSUbiquitousKeyValueStore.default
    /// Told what was brought in from another device, to show it.
    var brought: (SharedSetup.Applied) -> Void = { _ in }
    private var loop: Task<Void, Never>?
    private var observers: [NSObjectProtocol] = []

    init(store: Store) {
        self.store = store
    }

    func start() {
        let center = NotificationCenter.default
        observers.append(center.addObserver(forName: NSUbiquitousKeyValueStore.didChangeExternallyNotification,
                                            object: cloud, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.syncNow() }
        })
        observers.append(center.addObserver(forName: UIApplication.didBecomeActiveNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.cloud.synchronize()
                self.syncNow()
            }
        })
        cloud.synchronize()
        syncNow()
        loop = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: Self.every)
                self?.syncNow()
            }
        }
    }

    /// Merges this device's setup with iCloud's, puts the result in both
    /// places, and remembers it as the copy both now have.
    func syncNow() {
        guard let mine = try? SharedSetup.snapshot(store) else { return }
        let base = SharedSetup.decode(try? store.rawValue(SharedSetup.baseKey)) ?? [:]
        guard let theirs = SharedSetup.decode(cloud.string(forKey: Self.cloudKey)) else {
            // Nothing in iCloud yet: this device's setup goes there.
            cloud.set(SharedSetup.encode(mine), forKey: Self.cloudKey)
            keepBase(mine, was: base)
            return
        }
        let result = SharedSetup.merge(mine: mine, theirs: theirs, base: base)
        if result != mine {
            do {
                let applied = try SharedSetup.apply(result, over: mine, into: store)
                if applied.any { brought(applied) }
            } catch {
                Log.note("iCloud: could not bring in the shared setup: \(error.localizedDescription)")
                return
            }
        }
        if result != theirs { cloud.set(SharedSetup.encode(result), forKey: Self.cloudKey) }
        keepBase(result, was: base)
    }

    private func keepBase(_ shot: SharedSetup.Snapshot, was base: SharedSetup.Snapshot) {
        guard shot != base else { return }
        try? store.putRaw(SharedSetup.baseKey, SharedSetup.encode(shot))
    }
}
