import Foundation

/// Light or dark, saved as "dark" / "light" as the Rust build did.
enum Theme: String, Codable, Sendable {
    case dark
    case light
}

/// User preferences (kv `settings`). Every field has a default, so
/// settings saved by an older build still load.
struct Settings: Codable, Equatable, Sendable {
    static let key = "settings"

    var theme: Theme = .dark
    /// Index of the page shown when Haven was last closed.
    var lastPage: Int = 0

    enum CodingKeys: String, CodingKey {
        case theme
        case lastPage = "last_page"
    }

    init(theme: Theme = .dark, lastPage: Int = 0) {
        self.theme = theme
        self.lastPage = lastPage
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        theme = try c.decodeIfPresent(Theme.self, forKey: .theme) ?? .dark
        lastPage = try c.decodeIfPresent(Int.self, forKey: .lastPage) ?? 0
    }

    static func load(_ store: Store) throws -> Settings {
        try store.get(key) ?? Settings()
    }

    func save(_ store: Store) throws {
        try store.put(Self.key, self)
    }
}

/// The keys typed into Settings for the outside services Haven can use
/// (kv `keys`). Empty text means "not given". Never printed.
struct Keys: Codable, Equatable, Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    static let key = "keys"

    var tmdb = ""
    var traktID = ""
    var traktSecret = ""
    var simklID = ""
    /// OMDb (omdbapi.com): Rotten Tomatoes and Metacritic scores, the age rating.
    var omdb = ""

    enum CodingKeys: String, CodingKey {
        case tmdb
        case traktID = "trakt_id"
        case traktSecret = "trakt_secret"
        case simklID = "simkl_id"
        case omdb
    }

    init(tmdb: String = "", traktID: String = "", traktSecret: String = "", simklID: String = "", omdb: String = "") {
        self.tmdb = tmdb
        self.traktID = traktID
        self.traktSecret = traktSecret
        self.simklID = simklID
        self.omdb = omdb
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        tmdb = try c.decodeIfPresent(String.self, forKey: .tmdb) ?? ""
        traktID = try c.decodeIfPresent(String.self, forKey: .traktID) ?? ""
        traktSecret = try c.decodeIfPresent(String.self, forKey: .traktSecret) ?? ""
        simklID = try c.decodeIfPresent(String.self, forKey: .simklID) ?? ""
        omdb = try c.decodeIfPresent(String.self, forKey: .omdb) ?? ""
    }

    var description: String { "Keys { .. }" }
    var debugDescription: String { description }

    static func load(_ store: Store) throws -> Keys {
        try store.get(key) ?? Keys()
    }

    func save(_ store: Store) throws {
        try store.put(Self.key, self)
    }

    /// The keys to use: what was typed into Settings first, then the
    /// secrets file, then the keys built into this copy of Haven.
    func resolved(file: Secrets, builtIn: Secrets = .builtIn) -> Secrets {
        func pick(_ typed: String, _ filed: String?, _ built: String?) -> String? {
            let typed = typed.trimmingCharacters(in: .whitespacesAndNewlines)
            if !typed.isEmpty { return typed }
            if let filed { return filed }
            if let built, !built.isEmpty { return built }
            return nil
        }
        return Secrets(
            tmdbAPIKey: pick(tmdb, file.tmdbAPIKey, builtIn.tmdbAPIKey),
            traktClientID: pick(traktID, file.traktClientID, builtIn.traktClientID),
            traktClientSecret: pick(traktSecret, file.traktClientSecret, builtIn.traktClientSecret),
            simklClientID: pick(simklID, file.simklClientID, builtIn.simklClientID),
            omdbAPIKey: pick(omdb, file.omdbAPIKey, builtIn.omdbAPIKey))
    }
}

/// The add-ons, media servers and television sources the user switched
/// off (kv `switched_off`). They stay installed and signed in; Haven just
/// leaves them alone until they are switched on again.
struct SwitchedOff: Codable, Equatable, Sendable {
    static let key = "switched_off"

    /// Add-ons, by manifest address.
    var addons: [String] = []
    /// Media servers, by their number in the database.
    var servers: [Int64] = []
    /// Playlists and Xtream Codes accounts, by the address they are kept under.
    var tvSources: [String] = []

    enum CodingKeys: String, CodingKey {
        case addons
        case servers
        case tvSources = "tv_sources"
    }

    init(addons: [String] = [], servers: [Int64] = [], tvSources: [String] = []) {
        self.addons = addons
        self.servers = servers
        self.tvSources = tvSources
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        addons = try c.decodeIfPresent([String].self, forKey: .addons) ?? []
        servers = try c.decodeIfPresent([Int64].self, forKey: .servers) ?? []
        tvSources = try c.decodeIfPresent([String].self, forKey: .tvSources) ?? []
    }

    static func load(_ store: Store) throws -> SwitchedOff {
        try store.get(key) ?? SwitchedOff()
    }

    func save(_ store: Store) throws {
        try store.put(Self.key, self)
    }

    /// Puts `item` on a list of switched-off things, or takes it off.
    static func `switch`<T: Equatable>(_ list: inout [T], _ item: T, on: Bool) {
        list.removeAll { $0 == item }
        if !on { list.append(item) }
    }
}
