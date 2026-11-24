import Foundation

/// Which kind of Haven this is, decided when it is built.
///
/// - GitHub IPA (`ADDONS`): add-ons, media servers at any address.
/// - App Store / TestFlight (`STORE`): no add-on code or wording, media
///   servers only on the Local Area Network, Plex relay off.
enum Build {
    #if STORE
    static let isStore = true
    #else
    static let isStore = false
    #endif

    #if ADDONS
    static let hasAddons = true
    #else
    static let hasAddons = false
    #endif

    /// Media servers must resolve to the Local Area Network (or Tailscale).
    static let ownNetworkOnly = isStore

    static var version: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "?"
    }

    static var buildNumber: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "?"
    }
}

#if STORE && ADDONS
#error("The App Store build must not have add-ons.")
#endif
