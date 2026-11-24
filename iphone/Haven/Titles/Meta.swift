import Foundation

/// What can go wrong reading titles from a catalogue service.
enum TitlesError: Error, Equatable, LocalizedError {
    case invalidAddress(String)
    case http(HTTPError)
    case notText
    case badResponse(String)
    case unknownTitle

    var errorDescription: String? {
        switch self {
        case let .invalidAddress(address): "\"\(address)\" is not a web address"
        case let .http(error): error.errorDescription
        case .notText: "the answer was not text"
        case let .badResponse(why): "the answer could not be read: \(why)"
        case .unknownTitle: "this title is not known there"
        }
    }
}

/// The short form of a title that catalogues list: enough for a poster.
struct MetaPreview: Equatable, Hashable, Sendable, Codable {
    var id: String
    /// "movie", "series", …
    var kind: String
    var name: String
    var poster: String?
    /// Usually a year or a year range; some services send a number.
    var releaseInfo: String?
    /// Out of 10, as text ("8.4"), when the catalogue gives one.
    var rating: String?

    init(id: String, kind: String, name: String, poster: String? = nil, releaseInfo: String? = nil, rating: String? = nil) {
        self.id = id
        self.kind = kind
        self.name = name
        self.poster = poster
        self.releaseInfo = releaseInfo
        self.rating = rating
    }

    init(json o: JSONObject) throws {
        id = try o.string("id")
        kind = try o.string("type")
        name = try o.string("name")
        poster = try o.optionalString("poster")
        releaseInfo = o.textOrNumber("releaseInfo")
        rating = o.textOrNumber("imdbRating")
    }
}

/// Everything a service knows about one title.
struct MetaDetail: Equatable, Sendable, Codable {
    var id: String
    var kind: String
    var name: String
    var poster: String?
    /// A wide picture for the top of the page.
    var background: String?
    /// The title's name drawn as a picture, when there is one.
    var logo: String?
    var description: String?
    var releaseInfo: String?
    var runtime: String?
    var rating: String?
    /// `genres` now, `genre` in older services.
    var genres: [String] = []
    var cast: [String] = []
    var director: [String] = []
    /// When the title itself first came out, as an ISO date.
    var released: String?
    /// Episodes of a series (or the videos of a channel). Empty for films.
    var videos: [Video] = []
    /// The title's IMDb id (`tt…`) when its own id is something else.
    var imdbID: String?
    /// The title's number at TMDB, if known.
    var tmdbID: String?

    init(id: String, kind: String, name: String) {
        self.id = id
        self.kind = kind
        self.name = name
    }

    init(json o: JSONObject) throws {
        id = try o.string("id")
        kind = try o.string("type")
        name = try o.string("name")
        poster = try o.optionalString("poster")
        background = try o.optionalString("background")
        logo = try o.optionalString("logo")
        description = try o.optionalString("description")
        releaseInfo = o.textOrNumber("releaseInfo")
        runtime = o.textOrNumber("runtime")
        rating = o.textOrNumber("imdbRating")
        // Both spellings together: the current one counts.
        genres = o.raw["genres"] != nil ? o.texts("genres") : o.texts("genre")
        cast = o.texts("cast")
        director = o.texts("director")
        released = o.textOrNumber("released")
        if let list = o["videos"] {
            guard let items = list as? [Any] else { throw JSONObject.Wrong(field: "videos") }
            // Entries that are not objects or have no text id are left out.
            videos = try items.compactMap { item -> Video? in
                guard let video = JSONObject(item), video["id"] is String else { return nil }
                return try Video(json: video)
            }
        }
        imdbID = o.textOrNumber("imdb_id")
        tmdbID = o.textOrNumber("moviedb_id")
    }

    /// The seasons that have episodes, in order, specials (season 0) last.
    var seasons: [Int] {
        var seen = Set<Int>()
        return videos.compactMap(\.season)
            .sorted { ($0 == 0 ? 1 : 0, $0) < ($1 == 0 ? 1 : 0, $1) }
            .filter { seen.insert($0).inserted }
    }

    /// The episodes of one season, in episode order (ones with no number
    /// first, as the Rust build sorted them).
    func episodes(season: Int) -> [Video] {
        videos.enumerated()
            .filter { $0.element.season == season }
            .sorted { a, b in
                let (x, y) = (a.element.episode ?? -1, b.element.episode ?? -1)
                return x != y ? x < y : a.offset < b.offset
            }
            .map(\.element)
    }
}

/// One episode. Its `id` is what streams are asked for.
struct Video: Equatable, Sendable, Codable {
    var id: String
    /// `title` now, `name` in older services.
    var title: String?
    var season: Int?
    /// `episode` now, `number` in older services.
    var episode: Int?
    /// When it first aired, as an ISO date.
    var released: String?

    init(id: String, title: String? = nil, season: Int? = nil, episode: Int? = nil, released: String? = nil) {
        self.id = id
        self.title = title
        self.season = season
        self.episode = episode
        self.released = released
    }

    init(json o: JSONObject) throws {
        id = try o.string("id")
        // Both spellings together: the current one counts (even if null).
        title = o.raw["title"] != nil ? o.textOrNumber("title") : o.textOrNumber("name")
        season = o.number("season")
        episode = o.raw["episode"] != nil ? o.number("episode") : o.number("number")
        released = o.textOrNumber("released")
    }
}

/// Reading what catalogue services send, and telling titles apart.
enum Titles {
    /// Whether a name reads like a file's name ("Film (2024) [1080p]") and
    /// not like a title.
    static func isReleaseName(_ name: String) -> Bool {
        name.contains("[") || name.contains("(")
    }

    /// What two listings of the same film or series have in common: the
    /// kind, the name without what follows a bracket and without marks or
    /// case, and the year. The year comes from `year` (its first four
    /// digits) or, failing that, from a "(2024)" in the name.
    static func titleKey(kind: String, name: String, year: String?) -> String {
        let parts = name.split(omittingEmptySubsequences: false) { $0 == "(" || $0 == "[" }
        let first = parts.first.map { $0.trimmingCharacters(in: .whitespacesAndNewlines) } ?? ""
        let head = first.isEmpty ? name : first
        let plain = String(head.filter { $0.isLetter || $0.isNumber }).lowercased()
        func four(_ text: Substring) -> String? {
            let digits = text.utf8.prefix(4)
            guard digits.count == 4, digits.allSatisfy({ (48 ... 57).contains($0) }) else { return nil }
            return String(decoding: digits, as: UTF8.self)
        }
        let fromYear = year.flatMap { four(Substring($0)) }
        let fromName = parts.dropFirst().lazy.compactMap { part in
            four(part).flatMap { $0.hasPrefix("19") || $0.hasPrefix("20") ? $0 : nil }
        }.first
        return "\(kind)|\(plain)|\(fromYear ?? fromName ?? "")"
    }

    /// Drops repeats of the same film or series from a list, keeping each
    /// one's first place. Of two listings, the one named like a title wins
    /// over the one named like a file.
    static func withoutRepeats(_ titles: [MetaPreview]) -> [MetaPreview] {
        var kept: [(key: String, title: MetaPreview)] = []
        var places: [String: Int] = [:]
        for title in titles {
            let key = titleKey(kind: title.kind, name: title.name, year: title.releaseInfo)
            if let at = places[key] {
                if isReleaseName(kept[at].title.name) && !isReleaseName(title.name) {
                    kept[at].title = title
                }
            } else {
                places[key] = kept.count
                kept.append((key, title))
            }
        }
        return kept.map(\.title)
    }

    /// Reads a catalogue answer. Entries missing an id, type or name are
    /// left out instead of failing the whole catalogue.
    static func parseCatalog(_ json: String) throws -> [MetaPreview] {
        let page = try top(json)
        guard let metas = page["metas"] else { return [] }
        guard let list = metas as? [Any] else { throw TitlesError.badResponse("metas is not a list") }
        return list.compactMap { item in JSONObject(item).flatMap { try? MetaPreview(json: $0) } }
    }

    /// Reads a `meta` answer. Episodes with no id are left out.
    static func parseMeta(_ json: String) throws -> MetaDetail {
        let answer = try top(json)
        guard let meta = JSONObject(answer["meta"]), !meta.raw.isEmpty else {
            throw TitlesError.unknownTitle
        }
        do {
            return try MetaDetail(json: meta)
        } catch let wrong as JSONObject.Wrong {
            throw TitlesError.badResponse("\(wrong.field) has the wrong type")
        }
    }

    /// The top-level object of an answer.
    static func top(_ json: String) throws -> JSONObject {
        do {
            return try JSONObject.parse(json)
        } catch let wrong as JSONObject.Wrong {
            throw TitlesError.badResponse("\(wrong.field) is not an object")
        } catch {
            throw TitlesError.badResponse("not JSON")
        }
    }
}
