import Foundation

/// OMDb (omdbapi.com): the scores TMDB and Cinemeta do not have (Rotten
/// Tomatoes, Metacritic) and the age rating, for a title with an IMDb id.
/// Needs an OMDb key (one is built into Haven's builds; the free key allows
/// 1,000 requests a day, so answers are kept for 7 days).
enum OMDb {
    static let api = "https://www.omdbapi.com/"

    /// What OMDb knows about a title. Empty fields are not known. Saved
    /// with the Rust build's field names.
    struct Scores: Codable, Equatable, Sendable {
        /// "8.4"
        var imdb: String?
        /// "94%"
        var rottenTomatoes: String?
        /// "78" (out of 100)
        var metacritic: String?
        /// "R", "PG-13", "TV-MA"…
        var rated: String?
        /// "173 min"
        var runtime: String?

        enum CodingKeys: String, CodingKey {
            case imdb
            case rottenTomatoes = "rotten_tomatoes"
            case metacritic
            case rated
            case runtime
        }
    }

    /// Text that is there and not "N/A".
    private static func known(_ value: Any?) -> String? {
        guard let text = (value as? String)?.trimmingCharacters(in: .whitespacesAndNewlines),
              !text.isEmpty, text != "N/A" else { return nil }
        return text
    }

    static func readScores(_ answer: [String: Any]) -> Scores? {
        if answer["Response"] as? String == "False" { return nil }
        let ratings = answer["Ratings"] as? [Any] ?? []
        let rottenTomatoes = ratings.lazy.compactMap { item -> String? in
            guard let rating = item as? [String: Any], rating["Source"] as? String == "Rotten Tomatoes" else { return nil }
            return known(rating["Value"])
        }.first
        let rated = known(answer["Rated"]).flatMap { $0 == "Not Rated" || $0 == "Unrated" ? nil : $0 }
        return Scores(imdb: known(answer["imdbRating"]), rottenTomatoes: rottenTomatoes,
                      metacritic: known(answer["Metascore"]), rated: rated, runtime: known(answer["Runtime"]))
    }

    /// The scores for the title with IMDb id `imdb`; nil when OMDb does not
    /// know it.
    static func scores(key: String, imdb: String, http: HTTP = .shared) async throws -> Scores? {
        guard TMDB.isIMDbID(imdb) else { return nil }
        let key = key.filter { $0.isASCII && ($0.isLetter || $0.isNumber) }
        let answer: HTTPAnswer
        do {
            answer = try await http.send("\(api)?i=\(imdb)&apikey=\(key)", limit: TMDB.answerLimit)
        } catch let error as HTTPError {
            throw MetadataError.http(error)
        }
        switch answer.status {
        case 200 ..< 300:
            guard let json = try? JSONSerialization.jsonObject(with: answer.body) as? [String: Any] else {
                throw MetadataError.badAnswer
            }
            return readScores(json)
        case 401: throw MetadataError.badKey
        default: throw MetadataError.status(answer.status)
        }
    }
}
