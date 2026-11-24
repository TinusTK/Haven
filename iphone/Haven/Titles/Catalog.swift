import Foundation

/// One browsable list a catalogue service offers, such as "Popular" films.
struct Catalog: Equatable, Sendable {
    /// Content type: `movie`, `series`, `channel`, `tv`…
    var kind: String
    var id: String
    var name: String?
    var extra: [Extra] = []
    /// Older way of naming the extras a catalogue accepts.
    var extraSupported: [String] = []
    /// Older way of naming required extras.
    var extraRequired: [String] = []

    init(kind: String, id: String, name: String? = nil, extra: [Extra] = [],
         extraSupported: [String] = [], extraRequired: [String] = []) {
        self.kind = kind
        self.id = id
        self.name = name
        self.extra = extra
        self.extraSupported = extraSupported
        self.extraRequired = extraRequired
    }

    init(json o: JSONObject) throws {
        kind = try o.string("type")
        id = try o.string("id")
        name = try o.optionalString("name")
        if let list = o["extra"] {
            guard let items = list as? [Any] else { throw JSONObject.Wrong(field: "extra") }
            extra = try items.map { item in
                guard let object = JSONObject(item) else { throw JSONObject.Wrong(field: "extra") }
                return try Extra(json: object)
            }
        }
        extraSupported = try o.strings("extraSupported")
        extraRequired = try o.strings("extraRequired")
    }

    private var required: [String] {
        extra.filter(\.isRequired).map(\.name) + extraRequired
    }

    /// True when the catalogue only answers with extra input (a search
    /// term, a genre…), so it cannot be shown as a plain shelf.
    var needsInput: Bool { !required.isEmpty }

    /// True when the catalogue can be searched with nothing but a search term.
    var searchable: Bool {
        let supports = extra.contains { $0.name == "search" } || extraSupported.contains("search")
        return supports && required.allSatisfy { $0 == "search" }
    }

    /// The genres this catalogue can be narrowed to; empty if it has none.
    var genres: [String] { extra.first { $0.name == "genre" }?.options ?? [] }

    /// True when the catalogue hands out further pages (`skip`).
    var pages: Bool { extra.contains { $0.name == "skip" } || extraSupported.contains("skip") }

    /// True when the catalogue can be browsed with at most a genre chosen.
    var browsable: Bool { required.allSatisfy { $0 == "genre" && !genres.isEmpty } }

    /// What this catalogue's content is called: "films", "series"…
    var kindLabel: String {
        switch kind {
        case "movie": "films"
        case "series": "series"
        case "channel": "channels"
        case "tv": "TV"
        default: kind
        }
    }

    /// A shelf title such as "Popular films".
    var title: String {
        if let name = name?.trimmingCharacters(in: .whitespacesAndNewlines), !name.isEmpty {
            return "\(name) \(kindLabel)"
        }
        return Self.capitalised(kindLabel)
    }

    /// A title for this catalogue's search results, such as "Films".
    var resultsTitle: String { Self.capitalised(kindLabel) }

    private static func capitalised(_ text: String) -> String {
        guard let first = text.first, first.isASCII else { return text }
        return first.uppercased() + text.dropFirst()
    }

    /// Reads the catalogues a service lists in its description
    /// (`{"catalogs": [...]}`); other fields are ignored.
    static func parseList(_ json: String) throws -> [Catalog] {
        let described = try Titles.top(json)
        guard let list = described["catalogs"] else { return [] }
        do {
            guard let items = list as? [Any] else { throw JSONObject.Wrong(field: "catalogs") }
            return try items.map { item in
                guard let object = JSONObject(item) else { throw JSONObject.Wrong(field: "catalogs") }
                return try Catalog(json: object)
            }
        } catch let wrong as JSONObject.Wrong {
            throw TitlesError.badResponse("\(wrong.field) has the wrong type")
        }
    }
}

/// An option a catalogue accepts, such as `search`, `genre` or `skip`.
struct Extra: Equatable, Sendable {
    var name: String
    var isRequired = false
    /// The values it takes, such as the genres to choose from.
    var options: [String] = []

    init(name: String, isRequired: Bool = false, options: [String] = []) {
        self.name = name
        self.isRequired = isRequired
        self.options = options
    }

    init(json o: JSONObject) throws {
        name = try o.string("name")
        isRequired = try o.bool("isRequired")
        options = o.texts("options")
    }
}

/// Addresses of what a service serves next to its description
/// (`…/manifest.json`).
enum ResourceURL {
    /// Percent-encodes one part of a path the way catalogue services
    /// expect: everything except letters, digits and `-_.~`.
    static func encoded(_ part: String) -> String {
        var out = ""
        for byte in part.utf8 {
            let c = Character(UnicodeScalar(byte))
            if byte < 128 && (c.isLetter || c.isNumber || "-_.~".contains(c)) {
                out.append(c)
            } else {
                out += String(format: "%%%02X", byte)
            }
        }
        return out
    }

    /// `<resource>/<kind>/<id>.json`, or with extra options
    /// `<resource>/<kind>/<id>/<name>=<value>&<name>=<value>.json`.
    static func make(_ descriptionURL: String, resource: String, kind: String, id: String,
                     extras: [(String, String)] = []) throws -> String {
        let invalid = TitlesError.invalidAddress(descriptionURL.trimmingCharacters(in: .whitespacesAndNewlines))
        guard let url = try? HTTP.webURL(descriptionURL),
              var parts = URLComponents(url: url, resolvingAgainstBaseURL: false),
              parts.percentEncodedPath.hasSuffix("manifest.json")
        else { throw invalid }
        var path = String(parts.percentEncodedPath.dropLast("manifest.json".count))
        path += "\(encoded(resource))/\(encoded(kind))/\(encoded(id))"
        for (at, (name, value)) in extras.enumerated() {
            path += at == 0 ? "/" : "&"
            path += "\(encoded(name))=\(encoded(value))"
        }
        path += ".json"
        parts.percentEncodedPath = path
        guard let made = parts.string else { throw invalid }
        return made
    }

    /// The address of one of the service's catalogues, optionally searched.
    static func catalog(_ descriptionURL: String, _ catalog: Catalog, search: String? = nil) throws -> String {
        try make(descriptionURL, resource: "catalog", kind: catalog.kind, id: catalog.id,
                 extras: search.map { [("search", $0)] } ?? [])
    }
}
