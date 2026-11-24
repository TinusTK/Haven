import Foundation

// TMDB (themoviedb.org): extra details for titles with an IMDb id, and
// Haven's catalogue of films and series in the same shapes catalogue
// services use, so shelves, Explore and a title's page treat them like any
// other title. A TMDB title has the id `tmdb:movie:<n>` or `tmdb:tv:<n>`.
// Needs the user's own TMDB key: none is built in.
//
// This product uses the TMDB API but is not endorsed or certified by TMDB.

/// What can go wrong asking TMDB (or OMDb).
enum MetadataError: Error, Equatable, LocalizedError {
    case http(HTTPError)
    case badKey
    case status(Int)
    case badAnswer

    var errorDescription: String? {
        switch self {
        case let .http(error): error.errorDescription
        case .badKey: "TMDB did not accept the API key"
        case let .status(code): "TMDB answered with error \(code)"
        case .badAnswer: "TMDB's answer could not be read"
        }
    }
}

/// What TMDB knows about a title. Empty fields are simply not known.
struct TMDBDetails: Equatable, Sendable {
    var overview: String?
    var runtimeMinutes: Int?
    /// Out of 10.
    var rating: Double?
    var genres: [String] = []
    var cast: [String] = []
    /// A film's directors, or a series' creators.
    var directors: [String] = []
    /// Address of a wide picture.
    var backdrop: String?
    /// Address of the poster.
    var poster: String?
}

/// The ready-made lists TMDB keeps up to date.
enum TMDBList: Sendable {
    /// What people are watching this week.
    case trending
    case popular
    case topRated
    /// Films in cinemas now; series with episodes airing now.
    case now
    /// Films about to come out (films only).
    case upcoming
}

/// A genre as TMDB numbers it. Films and series have separate lists.
struct Genre: Equatable, Sendable {
    var id: Int64
    var name: String
}

/// A service that carries a title, and the ways it offers it.
struct Service: Equatable, Sendable {
    var name: String
    /// "Subscription", "Free", "Free with ads", "Rent", "Buy".
    var ways: [String]
}

/// Where a title can be watched in one country. The data is JustWatch's,
/// passed on by TMDB, and must be credited to JustWatch where shown.
struct Offers: Equatable, Sendable {
    /// The country's two-letter code, upper case.
    var country: String
    /// TMDB's page for the title in that country.
    var link: String?
    var services: [Service]
}

/// Talks to TMDB with the user's key.
struct TMDB: Sendable {
    static let api = "https://api.themoviedb.org/3"
    static let images = "https://image.tmdb.org/t/p"
    static let answerLimit = 4 * 1024 * 1024
    static let castKept = 10
    /// TMDB takes at most this many extra parts in one request.
    static let seasonsAtOnce = 20

    let key: String
    var api = TMDB.api
    var images = TMDB.images
    var http: HTTP = .shared

    /// `key` is a TMDB "API key" or the longer "API read access token";
    /// either works. Both are letters, digits and `-_.`.
    init(key: String, http: HTTP = .shared) {
        self.key = key.filter { $0.isASCII && ($0.isLetter || $0.isNumber || "-_.".contains($0)) }
        self.http = http
    }

    // MARK: Ids

    /// The id Haven gives a TMDB title.
    static func titleID(series: Bool, number: Int64) -> String {
        "tmdb:\(series ? "tv" : "movie"):\(number)"
    }

    /// Splits `tmdb:movie:<n>` or `tmdb:tv:<n>` into "is a series" and the
    /// number; nil for any other id.
    static func splitID(_ id: String) -> (series: Bool, number: Int64)? {
        guard id.hasPrefix("tmdb:") else { return nil }
        let rest = id.dropFirst("tmdb:".count)
        guard let colon = rest.firstIndex(of: ":") else { return nil }
        let series: Bool
        switch rest[..<colon] {
        case "tv": series = true
        case "movie": series = false
        default: return nil
        }
        guard let number = UInt64(rest[rest.index(after: colon)...]), number <= UInt64(Int64.max) else { return nil }
        return (series, Int64(number))
    }

    /// Whether an id is an IMDb id (`tt` and 1 to 12 digits).
    static func isIMDbID(_ id: String) -> Bool {
        guard id.hasPrefix("tt") else { return false }
        let digits = id.utf8.dropFirst(2)
        return !digits.isEmpty && digits.count <= 12 && digits.allSatisfy { (48 ... 57).contains($0) }
    }

    // MARK: Asking

    private func get(_ path: String) async throws -> [String: Any] {
        // The long token is sent as a header, the short key in the address.
        var url = api + path
        var headers: [(String, String)] = []
        if key.contains(".") {
            headers.append(("Authorization", "Bearer \(key)"))
        } else {
            url += (path.contains("?") ? "&" : "?") + "api_key=\(key)"
        }
        let answer: HTTPAnswer
        do {
            answer = try await http.send(url, headers: headers, limit: Self.answerLimit)
        } catch let error as HTTPError {
            throw MetadataError.http(error)
        }
        switch answer.status {
        case 200 ..< 300:
            guard let json = try? JSONSerialization.jsonObject(with: answer.body) as? [String: Any] else {
                throw MetadataError.badAnswer
            }
            return json
        case 401: throw MetadataError.badKey
        default: throw MetadataError.status(answer.status)
        }
    }

    private static func kind(_ series: Bool) -> String { series ? "tv" : "movie" }

    /// One page (20 titles) of a ready-made list; pages count from 1.
    func list(_ list: TMDBList, series: Bool, page: Int) async throws -> [MetaPreview] {
        let kind = Self.kind(series)
        let path = switch list {
        case .trending: "/trending/\(kind)/week"
        case .popular: "/\(kind)/popular"
        case .topRated: "/\(kind)/top_rated"
        case .now: series ? "/tv/on_the_air" : "/movie/now_playing"
        case .upcoming: "/movie/upcoming"
        }
        return Self.readPage(try await get("\(path)?page=\(max(page, 1))"), series: series, images: images)
    }

    /// One page of every film or series, most popular first, narrowed to a
    /// genre if one is given.
    func discover(series: Bool, genre: Int64?, page: Int) async throws -> [MetaPreview] {
        var path = "/discover/\(Self.kind(series))?sort_by=popularity.desc&page=\(max(page, 1))"
        if let genre { path += "&with_genres=\(genre)" }
        return Self.readPage(try await get(path), series: series, images: images)
    }

    func genres(series: Bool) async throws -> [Genre] {
        Self.readGenres(try await get("/genre/\(Self.kind(series))/list"))
    }

    /// Films or series whose name matches `words`.
    func search(series: Bool, words: String) async throws -> [MetaPreview] {
        let found = try await get("/search/\(Self.kind(series))?query=\(Self.escaped(words))")
        return Self.readPage(found, series: series, images: images)
    }

    /// The TMDB number of the title with that IMDb id, if TMDB knows it.
    func numberOf(series: Bool, imdbID: String) async throws -> Int64? {
        guard Self.isIMDbID(imdbID) else { return nil }
        return Self.foundID(try await get("/find/\(imdbID)?external_source=imdb_id"), series: series)
    }

    /// The services that carry a title, in the first of `countries`
    /// (two-letter codes, most wanted first) that has any.
    func offers(series: Bool, number: Int64, countries: [String]) async throws -> Offers? {
        Self.readOffers(try await get("/\(Self.kind(series))/\(number)/watch/providers"), countries: countries)
    }

    /// Everything about one title; for a series, with all its episodes.
    func title(series: Bool, number: Int64) async throws -> MetaDetail {
        let kind = Self.kind(series)
        let record = try await get("/\(kind)/\(number)?append_to_response=credits,external_ids")
        guard var meta = Self.readTitle(record, series: series, number: number, images: images) else {
            throw MetadataError.badAnswer
        }
        if series {
            let seasons = Self.seasonNumbers(record)
            for start in stride(from: 0, to: seasons.count, by: Self.seasonsAtOnce) {
                let group = seasons[start ..< min(start + Self.seasonsAtOnce, seasons.count)]
                let parts = group.map { "season/\($0)" }.joined(separator: ",")
                // A season that cannot be fetched is left out, not fatal.
                guard let answer = try? await get("/tv/\(number)?append_to_response=\(parts)") else { continue }
                for season in group {
                    meta.videos += Self.readSeason(answer, meta: meta, number: number, season: season)
                }
            }
        }
        return meta
    }

    /// Looks a title up by its IMDb id: TMDB's details for filling in what
    /// the title's own source lacks. nil when TMDB does not know it.
    func details(series: Bool, imdbID: String) async throws -> TMDBDetails? {
        guard let number = try await numberOf(series: series, imdbID: imdbID) else { return nil }
        return Self.readDetails(try await get("/\(Self.kind(series))/\(number)?append_to_response=credits"), images: images)
    }

    // MARK: Reading TMDB's answers

    /// Percent-encodes text for use inside a query string.
    static func escaped(_ text: String) -> String {
        ResourceURL.encoded(text.trimmingCharacters(in: .whitespacesAndNewlines))
    }

    static func text(_ value: Any?, _ key: String) -> String? {
        guard let text = (value as? [String: Any])?[key] as? String else { return nil }
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    /// A whole, non-negative number (serde's as_u64).
    static func whole(_ value: Any?) -> Int64? {
        guard let number = value as? NSNumber, !JSONObject.isBool(number), !JSONObject.isFloat(number) else { return nil }
        let n = number.int64Value
        return n >= 0 ? n : nil
    }

    /// Any number (serde's as_f64).
    static func real(_ value: Any?) -> Double? {
        guard let number = value as? NSNumber, !JSONObject.isBool(number) else { return nil }
        return number.doubleValue
    }

    static func list(_ value: Any?) -> [Any] { value as? [Any] ?? [] }

    /// "2019-05-04" to "2019".
    static func year(_ date: String?) -> String? {
        guard let date, date.utf8.count >= 4 else { return nil }
        return String(date.prefix(4))
    }

    static func oneDecimal(_ value: Double) -> String { String(format: "%.1f", value) }

    static func foundID(_ found: [String: Any], series: Bool) -> Int64? {
        whole((list(found[series ? "tv_results" : "movie_results"]).first as? [String: Any])?["id"])
    }

    /// A list answer as posters. Entries with no id or name are left out.
    static func readPage(_ found: [String: Any], series: Bool, images: String) -> [MetaPreview] {
        list(found["results"]).compactMap { item in
            guard let entry = item as? [String: Any], let id = whole(entry["id"]),
                  let name = text(entry, "title") ?? text(entry, "name")
            else { return nil }
            return MetaPreview(
                id: titleID(series: series, number: id), kind: series ? "series" : "movie", name: name,
                poster: text(entry, "poster_path").map { "\(images)/w342\($0)" },
                releaseInfo: year(text(entry, "release_date") ?? text(entry, "first_air_date")),
                rating: real(entry["vote_average"]).flatMap { $0 > 0 ? oneDecimal($0) : nil })
        }
    }

    static func readGenres(_ found: [String: Any]) -> [Genre] {
        list(found["genres"]).compactMap { item in
            guard let id = whole((item as? [String: Any])?["id"]), let name = text(item, "name") else { return nil }
            return Genre(id: id, name: name)
        }
    }

    static func readOffers(_ answer: [String: Any], countries: [String]) -> Offers? {
        guard let results = answer["results"] as? [String: Any] else { return nil }
        let ways = [("flatrate", "Subscription"), ("free", "Free"), ("ads", "Free with ads"), ("rent", "Rent"), ("buy", "Buy")]
        for wanted in countries {
            let country = wanted.trimmingCharacters(in: .whitespacesAndNewlines).uppercased()
            guard let entry = results[country] as? [String: Any] else { continue }
            var services: [Service] = []
            for (key, way) in ways {
                for name in list(entry[key]).compactMap({ text($0, "provider_name") }) {
                    if let at = services.firstIndex(where: { $0.name == name }) {
                        services[at].ways.append(way)
                    } else {
                        services.append(Service(name: name, ways: [way]))
                    }
                }
            }
            if !services.isEmpty { return Offers(country: country, link: text(entry, "link"), services: services) }
        }
        return nil
    }

    /// The numbers of a series' seasons that have episodes, specials included.
    static func seasonNumbers(_ record: [String: Any]) -> [Int] {
        list(record["seasons"]).compactMap { item in
            let season = item as? [String: Any]
            guard (whole(season?["episode_count"]) ?? 0) > 0, let number = whole(season?["season_number"]) else { return nil }
            return Int(number)
        }
    }

    static func readDetails(_ record: [String: Any], images: String = TMDB.images) -> TMDBDetails {
        func names(_ value: Any?) -> [String] {
            list(value).compactMap { ($0 as? [String: Any])?["name"] as? String }
        }
        let credits = record["credits"] as? [String: Any]
        var directors = list(credits?["crew"]).compactMap { member -> String? in
            let member = member as? [String: Any]
            guard member?["job"] as? String == "Director" else { return nil }
            return member?["name"] as? String
        }
        if directors.isEmpty { directors = names(record["created_by"]) }
        let runtime = whole(record["runtime"]) ?? whole(list(record["episode_run_time"]).first)
        return TMDBDetails(
            overview: text(record, "overview"),
            runtimeMinutes: runtime.flatMap { $0 > 0 ? Int($0) : nil },
            rating: real(record["vote_average"]).flatMap { $0 > 0 ? $0 : nil },
            genres: names(record["genres"]),
            cast: Array(names(credits?["cast"]).prefix(castKept)),
            directors: directors,
            backdrop: text(record, "backdrop_path").map { "\(images)/w1280\($0)" },
            poster: text(record, "poster_path").map { "\(images)/w500\($0)" })
    }

    /// A title's own record as a page of details (episodes come separately).
    static func readTitle(_ record: [String: Any], series: Bool, number: Int64, images: String) -> MetaDetail? {
        guard let name = text(record, "title") ?? text(record, "name") else { return nil }
        let details = readDetails(record, images: images)
        let released = text(record, "release_date") ?? text(record, "first_air_date")
        var meta = MetaDetail(id: titleID(series: series, number: number), kind: series ? "series" : "movie", name: name)
        meta.poster = details.poster
        meta.background = details.backdrop
        meta.description = details.overview
        meta.releaseInfo = year(released)
        meta.released = released
        meta.runtime = details.runtimeMinutes.map { "\($0) min" }
        meta.rating = details.rating.map(oneDecimal)
        meta.genres = details.genres
        meta.cast = details.cast
        meta.director = details.directors
        meta.imdbID = (text(record, "imdb_id") ?? text(record["external_ids"], "imdb_id")).flatMap { isIMDbID($0) ? $0 : nil }
        meta.tmdbID = String(number)
        return meta
    }

    /// The episodes of one season from an answer that carries `season/<n>`.
    /// An episode's id is `<imdb id>:<season>:<episode>`, the form other
    /// sources know, or `tmdb:tv:<number>:<season>:<episode>` without one.
    static func readSeason(_ answer: [String: Any], meta: MetaDetail, number: Int64, season: Int) -> [Video] {
        let stem = meta.imdbID ?? titleID(series: true, number: number)
        return list((answer["season/\(season)"] as? [String: Any])?["episodes"]).compactMap { item in
            guard let episode = whole((item as? [String: Any])?["episode_number"]) else { return nil }
            return Video(id: "\(stem):\(season):\(episode)", title: text(item, "name"), season: season,
                         episode: Int(episode), released: text(item, "air_date"))
        }
    }
}
