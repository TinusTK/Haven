#if ADDONS
import Foundation

// Add-ons: web services that give Haven catalogues, details and streams,
// in the public Stremio add-on protocol. An add-on is an address that
// serves a `manifest.json` describing what it offers; everything else is a
// JSON file next to it (`catalog/<type>/<id>.json`, `meta/…`, `stream/…`).
//
// Only in the GitHub build. The App Store build has none of this code or
// its wording (scripts/store-check.sh checks).

enum AddonError: Error, Equatable, LocalizedError {
    case invalidAddress(String)
    case http(HTTPError)
    case badManifest(String)
    case incompleteManifest(String)
    case badResponse(String)
    case notText
    case unknownTitle

    init(_ error: TitlesError) {
        switch error {
        case let .invalidAddress(address): self = .invalidAddress(address)
        case let .http(error): self = .http(error)
        case .notText: self = .notText
        case let .badResponse(why): self = .badResponse(why)
        case .unknownTitle: self = .unknownTitle
        }
    }

    var errorDescription: String? {
        switch self {
        case let .invalidAddress(address): "\"\(address)\" is not an add-on address"
        case let .http(error): error.errorDescription
        case let .badManifest(why): "the add-on's manifest could not be read: \(why)"
        case let .incompleteManifest(field): "the add-on's manifest has no \(field)"
        case let .badResponse(why): "the add-on's answer could not be read: \(why)"
        case .notText: "the add-on's answer was not text"
        case .unknownTitle: "the add-on does not know this title"
        }
    }
}

/// What an add-on says about itself. Fields Haven does not use are
/// ignored, so the raw JSON is what gets stored.
struct AddonManifest: Equatable, Sendable {
    var id: String
    var name: String
    var version: String = ""
    var description: String?
    var logo: String?
    var types: [String] = []
    var catalogs: [Catalog] = []
    /// What the add-on can be asked for: `catalog`, `meta`, `stream`…
    var resources: [AddonResource] = []
    /// If set, the add-on only knows titles whose id starts with one of these.
    var idPrefixes: [String]?

    /// Reads a manifest; refuses one without an id or a name.
    static func parse(_ json: String) throws -> AddonManifest {
        let manifest: AddonManifest
        do {
            let o = try JSONObject.parse(json)
            manifest = try AddonManifest(json: o)
        } catch let wrong as JSONObject.Wrong {
            throw AddonError.badManifest(wrong.field == "(top)" ? "not an object" : "\(wrong.field) is missing or has the wrong type")
        } catch {
            throw AddonError.badManifest("not JSON")
        }
        if manifest.id.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            throw AddonError.incompleteManifest("id")
        }
        if manifest.name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            throw AddonError.incompleteManifest("name")
        }
        return manifest
    }

    private init(json o: JSONObject) throws {
        id = try o.string("id")
        name = try o.string("name")
        version = try o.optionalString("version") ?? ""
        description = try o.optionalString("description")
        logo = try o.optionalString("logo")
        types = try o.strings("types")
        if let list = o["catalogs"] {
            guard let items = list as? [Any] else { throw JSONObject.Wrong(field: "catalogs") }
            catalogs = try items.map { item in
                guard let object = JSONObject(item) else { throw JSONObject.Wrong(field: "catalogs") }
                return try Catalog(json: object)
            }
        }
        if let list = o["resources"] {
            guard let items = list as? [Any] else { throw JSONObject.Wrong(field: "resources") }
            resources = try items.map(AddonResource.init(json:))
        }
        idPrefixes = try o.optionalStrings("idPrefixes")
    }

    /// Whether this add-on can be asked for `resource` (`meta`, `stream`…)
    /// about the title `id` of type `kind`.
    func provides(_ resource: String, kind: String, id: String) -> Bool {
        guard let found = resources.first(where: { $0.name == resource }) else { return false }
        guard (found.types ?? types).contains(kind) else { return false }
        guard let prefixes = found.idPrefixes ?? idPrefixes else { return true }
        return prefixes.contains { id.hasPrefix($0) }
    }

    /// This add-on as a source of catalogues, details and streams.
    func source(url: String) -> Source {
        Source(
            url: url, name: name, catalogs: catalogs,
            serves: resources.map {
                Serves(resource: $0.name, types: $0.types ?? types, idPrefixes: $0.idPrefixes ?? idPrefixes)
            })
    }

    /// Turns what the user typed or pasted into the add-on's manifest
    /// address. Accepts a full `…/manifest.json` address, the add-on's base
    /// address, or a `stremio://` install link.
    static func manifestURL(_ input: String) throws -> String {
        let input = input.trimmingCharacters(in: .whitespacesAndNewlines)
        let invalid = AddonError.invalidAddress(input)
        guard !input.isEmpty else { throw invalid }
        let address = input.hasPrefix("stremio://") ? "https://" + input.dropFirst("stremio://".count) : input
        guard let url = try? HTTP.webURL(address),
              var parts = URLComponents(url: url, resolvingAgainstBaseURL: false),
              let host = parts.host, !host.isEmpty
        else { throw invalid }
        parts.fragment = nil
        if !parts.percentEncodedPath.hasSuffix("/manifest.json") {
            var path = parts.percentEncodedPath
            while path.hasSuffix("/") { path.removeLast() }
            parts.percentEncodedPath = path + "/manifest.json"
        }
        guard let made = parts.string else { throw invalid }
        return made
    }
}

/// One kind of answer an add-on gives. The short form (just a name) uses
/// the manifest's own types and id prefixes.
struct AddonResource: Equatable, Sendable {
    var name: String
    var types: [String]?
    var idPrefixes: [String]?

    init(json value: Any) throws {
        if let name = value as? String {
            self.name = name
            return
        }
        guard let o = JSONObject(value) else { throw JSONObject.Wrong(field: "resources") }
        name = try o.string("name")
        types = try o.optionalStrings("types")
        idPrefixes = try o.optionalStrings("idPrefixes")
    }
}

/// Talks to add-ons over the web. Their catalogues and details are read
/// with `Catalogues`, like the built-in catalogue's.
struct AddonClient: Sendable {
    static let manifestLimit = 1024 * 1024
    static let answerLimit = 8 * 1024 * 1024

    var catalogues = Catalogues()

    private func text(_ url: String, limit: Int) async throws -> String {
        do {
            return try await catalogues.text(url, limit: limit)
        } catch let error as TitlesError {
            throw AddonError(error)
        }
    }

    /// The add-on's manifest as the raw JSON it served (kept as is, so
    /// nothing the add-on declares is lost). Check it with `AddonManifest.parse`.
    func manifestJSON(_ manifestURL: String) async throws -> String {
        try await text(manifestURL, limit: Self.manifestLimit)
    }

    /// The add-on's streams for a film, or for one episode (`id` is then
    /// the episode's id).
    func streams(_ manifestURL: String, kind: String, id: String) async throws -> [MediaStream] {
        do {
            let url = try ResourceURL.make(manifestURL, resource: "stream", kind: kind, id: id)
            return try MediaStream.parseList(try await text(url, limit: Self.answerLimit))
        } catch let error as TitlesError {
            throw AddonError(error)
        }
    }
}

/// The installed add-ons: reading them from the database, installing,
/// switching on and off, removing.
struct InstalledAddons: Sendable {
    /// One installed add-on as the Add-ons page lists it.
    struct Row: Equatable, Sendable {
        var url: String
        var name: String
        var description: String
        var version: String
        /// Catalogues that can be shown as shelves.
        var catalogs: Int
        var enabled: Bool
    }

    var rows: [Row] = []
    /// Sources to ask: the switched-on add-ons, in install order.
    var sources: [Source] = []

    /// Reads the installed add-ons. The Cinemeta copy older versions
    /// installed as an add-on is removed (it is built in now); a manifest
    /// that no longer reads is skipped; a switched-off add-on is listed
    /// but never asked.
    static func load(_ store: Store) -> InstalledAddons {
        let stored: [StoredAddon]
        do {
            stored = try store.addons()
        } catch {
            Log.note("could not read the installed add-ons: \(error.localizedDescription)")
            stored = []
        }
        let off = (try? SwitchedOff.load(store).addons) ?? []
        var installed = InstalledAddons()
        for addon in stored {
            if addon.url == Cinemeta.url {
                try? store.removeAddon(url: addon.url)
                continue
            }
            let manifest: AddonManifest
            do {
                manifest = try AddonManifest.parse(addon.manifest)
            } catch {
                Log.note("skipping add-on \(Log.plainAddress(addon.url)): \(error.localizedDescription)")
                continue
            }
            let enabled = !off.contains(addon.url)
            installed.rows.append(Row(
                url: addon.url, name: manifest.name, description: manifest.description ?? "",
                version: manifest.version, catalogs: manifest.catalogs.filter { !$0.needsInput }.count,
                enabled: enabled))
            if enabled { installed.sources.append(manifest.source(url: addon.url)) }
        }
        return installed
    }

    /// What installing an address came to, in words for the Add-ons page.
    enum Outcome: Equatable, Sendable {
        case installed(name: String)
        /// Cinemeta is built in.
        case builtIn
        case failed(String)

        var message: String {
            switch self {
            case let .installed(name): "Installed \(name)."
            case .builtIn: "Cinemeta is built into Haven: there is no need to install it."
            case let .failed(why): why
            }
        }

        var isError: Bool {
            if case .failed = self { return true }
            return false
        }
    }

    /// Fetches the manifest at the address the user gave and installs it.
    static func install(_ address: String, store: Store, client: AddonClient = AddonClient()) async -> Outcome {
        let url: String
        do {
            url = try AddonManifest.manifestURL(address)
        } catch {
            return .failed("Could not install the add-on: \(error.localizedDescription).")
        }
        if url == Cinemeta.url { return .builtIn }
        let json: String
        let manifest: AddonManifest
        do {
            json = try await client.manifestJSON(url)
            manifest = try AddonManifest.parse(json)
        } catch {
            return .failed("Could not install the add-on: \(error.localizedDescription).")
        }
        do {
            try store.saveAddon(url: url, manifest: json)
        } catch {
            return .failed("Could not save the add-on: \(error.localizedDescription)")
        }
        return .installed(name: manifest.name)
    }

    /// Switches an add-on on or off; it stays installed.
    static func setEnabled(_ url: String, on: Bool, store: Store) throws {
        var off = try SwitchedOff.load(store)
        SwitchedOff.switch(&off.addons, url, on: on)
        try off.save(store)
    }

    /// Removes an add-on, and forgets that it was switched off.
    static func remove(_ url: String, store: Store) throws {
        try store.removeAddon(url: url)
        try setEnabled(url, on: true, store: store)
    }
}
#endif
