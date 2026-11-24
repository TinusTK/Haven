import Foundation

// Media servers: the user's own Jellyfin, Emby and Plex servers, shown in
// the same shapes catalogues use (posters, details, streams) so the rest
// of Haven does not care where a title comes from. A title on a server has
// the id `srv:<server>:<item>`, `<server>` being its number in the database.

enum ServerError: Error, Equatable, LocalizedError {
    case invalidAddress(String)
    case request(String)
    case signInRefused
    case status(Int)
    case badAnswer
    case tooLarge
    case unreachable
    case notOwnNetwork

    var errorDescription: String? {
        switch self {
        case let .invalidAddress(address): "\"\(address)\" is not a server address"
        case let .request(why): "could not reach the server: \(why)"
        case .signInRefused: "the user name or password was not accepted"
        case let .status(code): "the server answered with error \(code)"
        case .badAnswer: "the server's answer could not be read"
        case .tooLarge: "the server's answer was too large"
        case .unreachable: "none of this Plex server's addresses could be reached"
        case .notOwnNetwork: "Haven connects to servers on your home network or over Tailscale"
        }
    }
}

enum ServerKind: String, Sendable, CaseIterable {
    case jellyfin
    case emby
    case plex

    /// The name people know it by.
    var label: String {
        switch self {
        case .jellyfin: "Jellyfin"
        case .emby: "Emby"
        case .plex: "Plex"
        }
    }
}

/// A server the user signed in to. Never prints its token.
struct Server: Equatable, Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    /// Its number in Haven's database; 0 until it is saved.
    var id: Int64 = 0
    var kind: ServerKind
    var name: String
    /// Where it answers, without a trailing slash.
    var address: String
    /// The signed-in user's id on the server (unused for Plex).
    var userID: String = ""
    /// What the server gave in exchange for signing in.
    var token: String

    init(id: Int64 = 0, kind: ServerKind, name: String, address: String, userID: String = "", token: String) {
        self.id = id
        self.kind = kind
        self.name = name
        self.address = address
        self.userID = userID
        self.token = token
    }

    /// A stored server, if its kind is one Haven knows.
    init?(_ stored: StoredServer) {
        guard let kind = ServerKind(rawValue: stored.kind) else { return nil }
        self.init(id: stored.id, kind: kind, name: stored.name, address: stored.address,
                  userID: stored.userID, token: stored.token)
    }

    var stored: StoredServer {
        StoredServer(id: id, kind: kind.rawValue, name: name, address: address, userID: userID, token: token)
    }

    var description: String { "Server(id: \(id), kind: \(kind.rawValue), name: \(name), address: \(address), ..)" }
    var debugDescription: String { description }
}

/// One of a server's libraries that holds films or series.
struct MediaLibrary: Equatable, Sendable, Codable {
    var id: String
    var name: String
    /// A collection (a list someone put together, such as a catalogue on a
    /// server that gathers them) and not a whole library.
    var collection: Bool
}

/// A film or episode to look for on a server: the one Haven has open from
/// somewhere else (another server, a catalogue, TMDB).
struct Wanted: Equatable, Sendable {
    var series = false
    var name = ""
    /// The year it came out; for a series the year it began.
    var year: String?
    var imdb: String?
    var tmdb: String?
    /// For an episode: its season and its number in the season.
    var episode: (season: Int, number: Int)?

    static func == (a: Wanted, b: Wanted) -> Bool {
        a.series == b.series && a.name == b.name && a.year == b.year && a.imdb == b.imdb && a.tmdb == b.tmdb
            && a.episode?.season == b.episode?.season && a.episode?.number == b.episode?.number
    }
}

/// A title on a server that might be the wanted one.
struct Candidate: Equatable, Sendable {
    var id: String
    var name: String
    var year: String?
    var imdb: String?
    var tmdb: String?
}

/// Rules shared by every kind of server.
enum Servers {
    /// Titles fetched at once for a shelf of a whole library.
    static let listLength = 300
    static let answerLimit = 16 * 1024 * 1024

    /// The id Haven uses for an item on a server.
    static func titleID(server: Int64, item: String) -> String { "srv:\(server):\(item)" }

    /// Splits a `srv:<server>:<item>` id; nil for any other id.
    static func splitID(_ id: String) -> (server: Int64, item: String)? {
        guard id.hasPrefix("srv:") else { return nil }
        let rest = id.dropFirst(4)
        guard let colon = rest.firstIndex(of: ":"), let server = Int64(rest[..<colon]) else { return nil }
        let item = String(rest[rest.index(after: colon)...])
        return item.isEmpty ? nil : (server, item)
    }

    /// Tidies an address the user typed: adds `http://` when no scheme is
    /// given and drops trailing slashes. Only http and https are accepted.
    static func address(_ typed: String) throws -> String {
        let typed = typed.trimmingCharacters(in: .whitespacesAndNewlines)
        let withScheme = typed.contains("://") ? typed : "http://\(typed)"
        guard let url = try? HTTP.webURL(withScheme),
              var parts = URLComponents(url: url, resolvingAgainstBaseURL: false),
              let host = parts.host, !host.isEmpty
        else { throw ServerError.invalidAddress(typed) }
        parts.host = host.lowercased()
        parts.scheme = parts.scheme?.lowercased()
        guard var tidy = parts.string else { throw ServerError.invalidAddress(typed) }
        while tidy.hasSuffix("/") { tidy.removeLast() }
        return tidy
    }

    /// Percent-encodes text for use in an address.
    static func query(_ text: String) -> String { ResourceURL.encoded(text) }

    /// Letters and digits only, lower case: "The Film!" and "the film" agree.
    static func plain(_ name: String) -> String {
        String(name.filter { $0.isLetter || $0.isNumber }).lowercased()
    }

    /// The first four digits: "2008-2013" and "2008" agree.
    static func yearOf(_ text: String?) -> String? {
        guard let digits = text?.utf8.prefix(4), digits.count == 4,
              digits.allSatisfy({ (48 ... 57).contains($0) }) else { return nil }
        return String(decoding: digits, as: UTF8.self)
    }

    /// Picks the candidate that is the wanted title. Shared ids decide; if
    /// the two sides share no kind of id, the name and the year must agree.
    static func bestMatch(_ wanted: Wanted, _ candidates: [Candidate]) -> Candidate? {
        func same(_ a: String?, _ b: String?) -> Bool? {
            guard let a, let b else { return nil }
            return a.caseInsensitiveCompare(b) == .orderedSame
        }
        if let byID = candidates.first(where: {
            same(wanted.imdb, $0.imdb) == true || same(wanted.tmdb, $0.tmdb) == true
        }) {
            return byID
        }
        return candidates.first { candidate in
            // An id of the same kind that differs means another title.
            guard same(wanted.imdb, candidate.imdb) == nil, same(wanted.tmdb, candidate.tmdb) == nil,
                  !wanted.name.isEmpty, plain(wanted.name) == plain(candidate.name) else { return false }
            if let a = yearOf(wanted.year), let b = yearOf(candidate.year) { return a == b }
            return true
        }
    }

    /// A server that gathers titles from elsewhere (Trellis) names an item
    /// by what its source calls it: the id spells `<m or s>|<the source's
    /// id>|<type>` in hex. Its parts, if `item` is such an id.
    static func gatheredParts(_ item: String) -> (kind: String, id: String, form: String)? {
        guard let spelt = unhex(item) else { return nil }
        let parts = spelt.split(separator: "|", omittingEmptySubsequences: false).map(String.init)
        guard parts.count == 3, parts[0] == "m" || parts[0] == "s", !parts[1].isEmpty else { return nil }
        return (parts[0], parts[1], parts[2])
    }

    /// Hex back to text; nil if it is not hex of UTF-8 text.
    static func unhex(_ text: String) -> String? {
        guard text.utf8.count % 2 == 0, text.allSatisfy(\.isASCII) else { return nil }
        var bytes: [UInt8] = []
        var index = text.startIndex
        while index < text.endIndex {
            let next = text.index(index, offsetBy: 2)
            guard let byte = UInt8(text[index ..< next], radix: 16) else { return nil }
            bytes.append(byte)
            index = next
        }
        return String(bytes: bytes, encoding: .utf8)
    }

    static func hex(_ text: String) -> String {
        text.utf8.map { String(format: "%02x", $0) }.joined()
    }

    /// True for a gathered title not named by its IMDb id (a TMDB or Kitsu
    /// number, say). Sources that find streams go by IMDb ids, so asked for
    /// under that name such a title has none.
    static func wantsIMDb(_ item: String) -> Bool {
        guard let parts = gatheredParts(item) else { return false }
        return !parts.id.hasPrefix("tt")
    }

    /// The same gathered title named by its IMDb id; nil if `item` is not a
    /// gathered title's id or already goes by one.
    static func byIMDb(_ item: String, imdb: String) -> String? {
        guard let parts = gatheredParts(item), !parts.id.hasPrefix("tt"), imdb.hasPrefix("tt") else { return nil }
        return hex("\(parts.kind)|\(imdb)|\(parts.form)")
    }

    /// A picture's height in words: "4K", "1080p", "720p"…
    static func quality(width: Double?, height: Double?) -> String? {
        let (width, height) = (width ?? 0, height ?? 0)
        if width <= 0 && height <= 0 { return nil }
        if width >= 3800 || height >= 2000 { return "4K" }
        if width >= 1900 || height >= 1000 { return "1080p" }
        if width >= 1260 || height >= 700 { return "720p" }
        if height > 0 { return String(format: "%.0fp", height) }
        return "SD"
    }

    /// A file size in words: "4.2 GB", "700 MB"; nothing under 1 MB.
    static func size(_ bytes: Double) -> String? {
        let mb = 1024.0 * 1024.0
        if bytes < mb { return nil }
        if bytes >= 1024 * mb { return String(format: "%.1f GB", bytes / (1024 * mb)) }
        return String(format: "%.0f MB", bytes / mb)
    }
}

/// Reading server answers: text (trimmed; a number as text), numbers
/// (or text that reads as one) and lists.
enum ServerJSON {
    static func text(_ value: Any?, _ key: String) -> String? {
        switch (value as? [String: Any])?[key] {
        case let text as String:
            let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
            return trimmed.isEmpty ? nil : trimmed
        case let number as NSNumber where !JSONObject.isBool(number):
            return JSONObject.text(number)
        default:
            return nil
        }
    }

    static func number(_ value: Any?, _ key: String) -> Double? {
        switch (value as? [String: Any])?[key] {
        case let number as NSNumber where !JSONObject.isBool(number): number.doubleValue
        case let text as String: Double(text.trimmingCharacters(in: .whitespacesAndNewlines))
        default: nil
        }
    }

    static func list(_ value: Any?, _ key: String) -> [Any] {
        (value as? [String: Any])?[key] as? [Any] ?? []
    }

    static func object(_ value: Any?, _ key: String) -> [String: Any]? {
        (value as? [String: Any])?[key] as? [String: Any]
    }

    static func rating(_ value: Double) -> String { String(format: "%.1f", value) }

    /// A season or episode number, as Rust's `as u32` turned a number:
    /// not a number or below zero is 0, a fraction is cut off.
    static func whole(_ value: Double) -> Int {
        guard value.isFinite, value > 0 else { return 0 }
        return Int(min(value, Double(UInt32.max)))
    }
}
