import Foundation

/// One kind of answer a source gives, for which types and ids.
struct Serves: Equatable, Sendable {
    /// `catalog`, `meta`, `stream`…
    var resource: String
    var types: [String]
    /// If set, only ids that start with one of these.
    var idPrefixes: [String]?
}

/// A source of catalogues and details: Haven's built-in catalogue, or (in
/// the GitHub build) one the user installed.
struct Source: Equatable, Sendable {
    /// Its description's address; everything else it serves sits next to it.
    var url: String
    var name: String
    var catalogs: [Catalog]
    var serves: [Serves]

    /// Whether this source can be asked for `resource` about the title `id`
    /// of type `kind`.
    func provides(_ resource: String, kind: String, id: String) -> Bool {
        serves.contains { serves in
            serves.resource == resource
                && serves.types.contains(kind)
                && (serves.idPrefixes.map { prefixes in prefixes.contains { id.hasPrefix($0) } } ?? true)
        }
    }
}

/// Cinemeta: the free catalogue of films and series built into Haven. It
/// needs no key, so Home and Explore have something in them from the start.
enum Cinemeta {
    static let url = "https://v3-cinemeta.strem.io/manifest.json"
    static let name = "Cinemeta"
    /// Where its last description is kept (kv), as raw JSON text.
    static let keptKey = "cinemeta_description"

    /// The built-in catalogue, with the catalogues it lists (from its own
    /// description, or the plain list below when that is not known yet).
    static func source(_ catalogs: [Catalog]? = nil) -> Source {
        let types = ["movie", "series"]
        return Source(
            url: url, name: name, catalogs: catalogs ?? fallback,
            serves: ["catalog", "meta"].map { Serves(resource: $0, types: types, idPrefixes: ["tt"]) })
    }

    /// Popular and Featured films and series, searchable, with pages.
    static var fallback: [Catalog] {
        var list: [Catalog] = []
        for (id, name) in [("top", "Popular"), ("imdbRating", "Featured")] {
            for kind in ["movie", "series"] {
                list.append(Catalog(kind: kind, id: id, name: name,
                                    extraSupported: id == "top" ? ["skip", "search"] : ["skip"]))
            }
        }
        return list
    }

    /// The built-in catalogue as it was last described, if it was.
    static func kept(_ store: Store) -> Source {
        guard let json = try? store.get(keptKey, as: String.self),
              let catalogs = try? Catalog.parseList(json), !catalogs.isEmpty
        else { return source() }
        return source(catalogs)
    }

    /// Asks Cinemeta which catalogues it has now. Returns the new source
    /// (and keeps its description) only when it changed from `current`.
    static func refresh(_ store: Store, current: Source, catalogues: Catalogues = Catalogues()) async -> Source? {
        guard let json = try? await catalogues.descriptionJSON(url),
              let catalogs = try? Catalog.parseList(json), !catalogs.isEmpty
        else { return nil }
        let fresh = source(catalogs)
        guard fresh != current else { return nil }
        do {
            try store.put(keptKey, json)
        } catch {
            Log.note("could not keep the catalogue list: \(error.localizedDescription)")
        }
        return fresh
    }
}

/// Reads catalogues and details from a catalogue service over the web.
struct Catalogues: Sendable {
    static let descriptionLimit = 1024 * 1024
    static let answerLimit = 8 * 1024 * 1024

    var http: HTTP = .shared

    func text(_ url: String, limit: Int) async throws -> String {
        let body: Data
        do {
            body = try await http.getBytes(url, limit: limit)
        } catch let error as HTTPError {
            throw TitlesError.http(error)
        }
        guard let text = String(data: body, encoding: .utf8) else { throw TitlesError.notText }
        return text
    }

    /// The service's own description as raw JSON.
    func descriptionJSON(_ url: String) async throws -> String {
        try await text(url, limit: Self.descriptionLimit)
    }

    /// The first page of a catalogue, or of its search results.
    func catalog(_ sourceURL: String, _ catalog: Catalog, search: String? = nil) async throws -> [MetaPreview] {
        let url = try ResourceURL.catalog(sourceURL, catalog, search: search)
        return try Titles.parseCatalog(try await text(url, limit: Self.answerLimit))
    }

    /// A page of a catalogue: narrowed to `genre` if given, starting `skip`
    /// titles in.
    func catalogPage(_ sourceURL: String, _ catalog: Catalog, genre: String?, skip: Int) async throws -> [MetaPreview] {
        var extras: [(String, String)] = []
        if let genre { extras.append(("genre", genre)) }
        if skip != 0 { extras.append(("skip", String(skip))) }
        let url = try ResourceURL.make(sourceURL, resource: "catalog", kind: catalog.kind, id: catalog.id, extras: extras)
        return try Titles.parseCatalog(try await text(url, limit: Self.answerLimit))
    }

    /// Everything the source knows about one title.
    func meta(_ sourceURL: String, kind: String, id: String) async throws -> MetaDetail {
        let url = try ResourceURL.make(sourceURL, resource: "meta", kind: kind, id: id)
        return try Titles.parseMeta(try await text(url, limit: Self.answerLimit))
    }
}
