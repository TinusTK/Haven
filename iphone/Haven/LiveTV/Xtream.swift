import Foundation

// Xtream Codes: a provider's account (server address, user name and
// password) instead of a playlist address. The account's live channels are
// asked for through its `player_api.php` and turned into the same playlist
// text an M3U source gives, so the rest of Live TV does not care which kind
// a source is. Its films and series are offered as places to watch a title.

enum XtreamError: Error, Equatable, LocalizedError {
    case badAnswer
    case refused
    case notActive(String)

    var errorDescription: String? {
        switch self {
        case .badAnswer: "the server's answer was not an Xtream Codes answer"
        case .refused: "the user name or password was not accepted"
        case let .notActive(status): "the account is \(status)"
        }
    }
}

/// An Xtream Codes account. Never prints its password.
struct XtreamAccount: Equatable, Hashable, Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    /// Scheme, host and port, without a trailing slash.
    var server: String
    var username: String
    var password: String

    /// Tidies what the user typed. The server may be given with or without
    /// `http://`, and with a path (a pasted playlist or API address): only
    /// the scheme, host and port are kept. nil if any of the three parts is
    /// missing or the server is not a web address.
    init?(server: String, username: String, password: String) {
        let server = server.trimmingCharacters(in: .whitespacesAndNewlines)
        let username = username.trimmingCharacters(in: .whitespacesAndNewlines)
        let password = password.trimmingCharacters(in: .whitespacesAndNewlines)
        let scheme: String
        let rest: Substring
        if let mark = server.range(of: "://") {
            scheme = server[..<mark.lowerBound].lowercased()
            rest = server[mark.upperBound...]
        } else {
            scheme = "http"
            rest = Substring(server)
        }
        let full = rest.split(omittingEmptySubsequences: false) { $0 == "/" || $0 == "?" || $0 == "#" }.first ?? ""
        // "user:password@" in front of the host is not part of it.
        let host = full.split(separator: "@", omittingEmptySubsequences: false).last ?? full
        guard !host.isEmpty, !host.contains(where: \.isWhitespace), scheme == "http" || scheme == "https",
              !username.isEmpty, !password.isEmpty else { return nil }
        self.server = "\(scheme)://\(host)"
        self.username = username
        self.password = password
    }

    private static func encoded(_ text: String) -> String { ResourceURL.encoded(text) }

    /// Percent-decoding, with `+` as a space.
    private static func decoded(_ part: Substring) -> String {
        let bytes = Array(part.utf8)
        var out: [UInt8] = []
        var at = 0
        while at < bytes.count {
            if bytes[at] == UInt8(ascii: "%"), at + 2 < bytes.count,
               let byte = UInt8(String(decoding: bytes[at + 1 ... at + 2], as: UTF8.self), radix: 16) {
                out.append(byte)
                at += 3
            } else {
                out.append(bytes[at] == UInt8(ascii: "+") ? UInt8(ascii: " ") : bytes[at])
                at += 1
            }
        }
        return String(decoding: out, as: UTF8.self)
    }

    /// The address to ask: with no action, about the account itself.
    func api(_ action: String? = nil) -> String {
        var url = "\(server)/player_api.php?username=\(Self.encoded(username))&password=\(Self.encoded(password))"
        if let action { url += "&action=\(Self.encoded(action))" }
        return url
    }

    /// The address Haven stores the account under, as a source.
    var sourceURL: String { api() }

    /// Reads an account back from the address it was stored under; nil for
    /// any other address (an M3U playlist's).
    init?(sourceURL url: String) {
        guard let mark = url.range(of: "/player_api.php?") else { return nil }
        let server = String(url[..<mark.lowerBound])
        let pairs = url[mark.upperBound...].split(separator: "&", omittingEmptySubsequences: false)
        if pairs.contains(where: { $0.hasPrefix("action=") }) { return nil }
        func value(_ name: String) -> String? {
            for pair in pairs {
                guard let eq = pair.firstIndex(of: "=") else { continue }
                if pair[..<eq] == name { return Self.decoded(pair[pair.index(after: eq)...]) }
            }
            return nil
        }
        guard let username = value("username"), let password = value("password") else { return nil }
        self.init(server: server, username: username, password: password)
    }

    /// How the account is shown in lists: the user name and the host, never
    /// the password.
    var label: String { "\(username) at \(M3U.sourceName(server))" }

    /// Where one of the account's films plays from.
    func film(id: String, extension ext: String) -> String { file("movie", id: id, extension: ext) }

    /// Where one episode of one of the account's series plays from.
    func episode(id: String, extension ext: String) -> String { file("series", id: id, extension: ext) }

    private func file(_ kind: String, id: String, extension ext: String) -> String {
        // Only letters and digits make a file ending; anything else is left off.
        let ending = ext.filter { $0.isASCII && ($0.isLetter || $0.isNumber) }
        return "\(server)/\(kind)/\(Self.encoded(username))/\(Self.encoded(password))/\(Self.encoded(id)).\(ending.isEmpty ? "mp4" : ending)"
    }

    /// Where one live channel plays from.
    func stream(id: String, format: String) -> String {
        "\(server)/live/\(Self.encoded(username))/\(Self.encoded(password))/\(Self.encoded(id)).\(format)"
    }

    var description: String { "XtreamAccount(server: \(server), username: \(username), ..)" }
    var debugDescription: String { description }
}

/// What the server says about the account when signing in.
struct XtreamInfo: Equatable, Sendable {
    /// When the account runs out, in seconds since 1970; nil if never.
    var expires: Int64?
    /// The stream formats the account may use ("ts", "m3u8"…).
    var formats: [String] = []
}

/// A film or a series in an account's library, as its list names it.
struct OnDemand: Equatable, Sendable {
    /// The name as the provider wrote it ("EN - A Film (2021) 4K").
    var name: String
    /// A film's stream id, or a series' id.
    var id: String
    /// A film's file ending; empty for a series.
    var ext: String
    /// The year, where the list gives one.
    var year: String?
    /// Its number at TMDB, where the list gives one.
    var tmdb: String?
}

enum Xtream {
    private typealias J = ServerJSON

    private static func parse(_ json: String) -> Any? {
        try? JSONSerialization.jsonObject(with: Data(json.utf8), options: [.fragmentsAllowed])
    }

    /// Reads the answer to signing in (the API asked with no action).
    static func signIn(_ json: String) throws -> XtreamInfo {
        guard let user = J.object(parse(json), "user_info") else { throw XtreamError.badAnswer }
        guard J.text(user, "auth") == "1" else { throw XtreamError.refused }
        if let status = J.text(user, "status"), status.caseInsensitiveCompare("active") != .orderedSame {
            throw XtreamError.notActive(status.lowercased())
        }
        return XtreamInfo(
            expires: J.text(user, "exp_date").flatMap { Int64($0) },
            formats: (user["allowed_output_formats"] as? [Any] ?? []).compactMap { ($0 as? String)?.lowercased() })
    }

    /// The account's live channels, from its answers to `get_live_categories`
    /// and `get_live_streams`. Channels without an id or a name are left out;
    /// a missing or unreadable category list only costs the group names.
    static func channels(_ account: XtreamAccount, info: XtreamInfo, categories categoriesJSON: String,
                         streams streamsJSON: String) throws -> [Channel] {
        guard let streams = parse(streamsJSON) as? [Any] else { throw XtreamError.badAnswer }
        var categories: [String: String] = [:]
        for category in parse(categoriesJSON) as? [Any] ?? [] {
            if let id = J.text(category, "category_id"), let name = J.text(category, "category_name"), categories[id] == nil {
                categories[id] = name
            }
        }
        // The plain transport stream unless the account is only let have HLS.
        let onlyHLS = !info.formats.isEmpty && !info.formats.contains("ts") && info.formats.contains("m3u8")
        let format = onlyHLS ? "m3u8" : "ts"
        return streams.compactMap { stream in
            guard let id = J.text(stream, "stream_id"), let name = J.text(stream, "name") else { return nil }
            return Channel(name: name, address: account.stream(id: id, format: format),
                           group: J.text(stream, "category_id").flatMap { categories[$0] },
                           logo: J.text(stream, "stream_icon"), guideID: J.text(stream, "epg_channel_id"),
                           radio: J.text(stream, "stream_type")?.contains("radio") ?? false)
        }
    }

    private static func onDemand(_ json: String, idKey: String) throws -> [OnDemand] {
        guard let listed = parse(json) as? [Any] else { throw XtreamError.badAnswer }
        func year(_ text: String?) -> String? {
            guard let digits = text?.utf8.prefix(4), digits.count == 4, digits.allSatisfy({ (48 ... 57).contains($0) }) else { return nil }
            return String(decoding: digits, as: UTF8.self)
        }
        return listed.compactMap { entry in
            guard let name = J.text(entry, "name"), let id = J.text(entry, idKey) else { return nil }
            return OnDemand(
                name: name, id: id, ext: J.text(entry, "container_extension") ?? "",
                year: year(J.text(entry, "year")) ?? year(J.text(entry, "releaseDate")) ?? year(J.text(entry, "release_date")),
                tmdb: (J.text(entry, "tmdb") ?? J.text(entry, "tmdb_id")).flatMap { $0 == "0" ? nil : $0 })
        }
    }

    /// The account's films, from its answer to `get_vod_streams`.
    static func films(_ json: String) throws -> [OnDemand] { try onDemand(json, idKey: "stream_id") }

    /// The account's series, from its answer to `get_series`.
    static func series(_ json: String) throws -> [OnDemand] { try onDemand(json, idKey: "series_id") }

    private static func isYear(_ word: some StringProtocol) -> Bool {
        let bytes = Array(word.utf8)
        return bytes.count == 4 && bytes.allSatisfy { (48 ... 57).contains($0) } && (word.hasPrefix("19") || word.hasPrefix("20"))
    }

    /// A provider's name for a title, down to what tells titles apart: no
    /// country tag in front ("EN - "), nothing from a bracket on, no year or
    /// picture-quality words at the end; letters and digits only. With it
    /// comes a year found in the name, if any.
    static func plainTitle(_ name: String) -> (plain: String, year: String?) {
        let parts = name.split(omittingEmptySubsequences: false) { $0 == "(" || $0 == "[" }
        var year = parts.dropFirst().lazy.compactMap { part -> String? in
            let four = String(part.prefix(4))
            return four.utf8.count == 4 && isYear(four) ? four : nil
        }.first
        var head = (parts.first.map(String.init) ?? name).trimmingCharacters(in: .whitespaces)
        // "EN - ", "UK: ", "4K| " in front: a short tag in capitals, then a mark.
        let splits: [(Substring, Substring)?] = [
            head.range(of: " - ").map { (head[..<$0.lowerBound], head[$0.upperBound...]) },
            head.firstIndex(of: "|").map { (head[..<$0], head[head.index(after: $0)...]) },
            head.firstIndex(of: ":").map { (head[..<$0], head[head.index(after: $0)...]) },
        ]
        let tagged = splits.compactMap { $0 }.first { tag, rest in
            let tag = tag.trimmingCharacters(in: .whitespaces)
            return (1 ... 5).contains(tag.utf8.count)
                && tag.allSatisfy { ($0.isASCII && $0.isUppercase) || ($0.isASCII && $0.isNumber) || $0 == "+" }
                && tag.contains { $0.isASCII && $0.isUppercase }
                && !rest.trimmingCharacters(in: .whitespaces).isEmpty
        }
        if let (_, rest) = tagged { head = rest.trimmingCharacters(in: .whitespaces) }
        var words = head.split(whereSeparator: \.isWhitespace).map(String.init)
        let quality: Set<String> = ["4k", "uhd", "fhd", "hd", "sd", "hdr", "hevc", "multi", "1080p", "720p", "2160p", "480p"]
        while words.count > 1 {
            let last = words[words.count - 1].lowercased()
            if isYear(last) {
                if year == nil { year = last }
            } else if !quality.contains(last) {
                break
            }
            words.removeLast()
        }
        let plain = String(words.joined().filter { $0.isLetter || $0.isNumber }).lowercased()
        return (plain, year)
    }

    /// The entries of a list that are the wanted title: by TMDB number where
    /// both sides have one, else by name, and by year where both sides have
    /// one. A provider often lists a film several times (in other qualities
    /// or languages): all of them are returned.
    static func matches(_ listed: [OnDemand], name: String, year: String?, tmdb: String?) -> [OnDemand] {
        let wanted = plainTitle(name).plain
        let wantedYear = year.map { String($0.prefix(4)) }
        return listed.filter { entry in
            if let tmdb, let listedTMDB = entry.tmdb { return tmdb == listedTMDB }
            let (plain, yearInName) = plainTitle(entry.name)
            guard !wanted.isEmpty, plain == wanted else { return false }
            if let a = wantedYear, let b = entry.year ?? yearInName { return a == b }
            return true
        }
    }

    /// One episode from the account's answer to `get_series_info`: its id
    /// and file ending.
    static func episode(_ json: String, season: Int, episode: Int) -> (id: String, ext: String)? {
        guard let episodes = (parse(json) as? [String: Any])?["episodes"] else { return nil }
        // Seasons come as an object keyed by number, or as a list of lists.
        let seasons: [Any]
        if let keyed = episodes as? [String: Any] {
            seasons = keyed.keys.sorted().compactMap { keyed[$0] }
        } else if let list = episodes as? [Any] {
            seasons = list
        } else {
            return nil
        }
        for entry in seasons.compactMap({ $0 as? [Any] }).joined() {
            func number(_ key: String) -> Int? { J.text(entry, key).flatMap { UInt32($0) }.map(Int.init) }
            guard number("season") == season, number("episode_num") == episode else { continue }
            guard let id = J.text(entry, "id") else { continue }
            return (id, J.text(entry, "container_extension") ?? "")
        }
        return nil
    }
}
