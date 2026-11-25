import Foundation

/// "What's New": this version's notes, over Haven right after the loading
/// screen. It shows at every start after an update until "Don't show again"
/// is ticked; a fresh install never sees it. The notes are built in
/// (`whats-new.json`, the Rust build's file), so every build shows the
/// same ones. They must never mention add-ons (the App Store build).
enum WhatsNew {
    /// Which version's notes this device has put away (kv).
    static let seenKey = "whats_new_seen"

    struct Notes: Equatable, Sendable {
        var version: String
        var sections: [Section]

        struct Section: Equatable, Sendable, Identifiable {
            var title: String
            var lines: [String]
            var id: String { title }
        }
    }

    static var builtIn: Notes? {
        guard let url = Bundle.main.url(forResource: "whats-new", withExtension: "json"),
              let text = try? String(contentsOf: url, encoding: .utf8) else { return nil }
        return parse(text)
    }

    static func parse(_ text: String) -> Notes? {
        guard let o = try? JSONObject.parse(text), let version = o["version"] as? String,
              let sections = o["sections"] as? [Any] else { return nil }
        return Notes(version: version, sections: sections.compactMap { item in
            guard let section = item as? [String: Any], let title = section["title"] as? String,
                  let lines = section["lines"] as? [Any] else { return nil }
            return Notes.Section(title: title, lines: lines.compactMap { $0 as? String })
        })
    }

    /// Whether the notes show at this start. `existing` is false on a fresh
    /// install (no settings saved yet): those notes then count as seen.
    static func due(_ notes: Notes?, store: Store, existing: Bool) -> Bool {
        guard let notes else { return false }
        guard existing else {
            try? store.put(seenKey, notes.version)
            return false
        }
        return (try? store.get(seenKey, as: String.self)) != notes.version
    }

    /// Done was pressed: with "Don't show again" ticked, these notes are put away.
    static func done(_ notes: Notes?, store: Store, dontShowAgain: Bool) {
        guard dontShowAgain, let notes else { return }
        try? store.put(seenKey, notes.version)
    }
}
