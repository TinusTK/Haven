import Foundation

/// Everything the player needs to play a stream and remember the place.
struct PlayRequest: Equatable, Sendable {
    var address: String
    var title: String
    /// "S1 E2 · Name"; empty for a film.
    var detail: String
    var kind: String
    var titleID: String
    var videoID: String
    var poster: String?
    /// Remember how far it was watched (films and episodes, not channels).
    var remember = true
    /// Where it plays from, in words, for the player's information.
    var source: String
    /// The episode after this one, in words; nil if there is none.
    var next: String?
    /// The media server item (`srv:…`) it plays, told how far it was watched.
    var serverItem: String?
}

/// One episode in the title page's list.
struct EpisodeRow: Equatable, Sendable {
    var id: String
    /// "E5"; empty when it has no number.
    var number: String
    var title: String
    /// "2019-01-27"
    var date: String
    var selected = false
    /// Watched to the end.
    var watched = false
}

/// One place to watch in the title page's list.
struct StreamRow: Equatable, Sendable, Identifiable {
    /// Stays the same while rows are added around it.
    var id: Int
    /// The media server or add-on that answered.
    var source: String
    var name: String
    var detail: String
    /// The rest of the detail, on a second line.
    var detail2: String
    /// The file's size ("12.3 GB"); empty if unknown.
    var size: String
    /// How many are sharing it; empty if unknown.
    var peers: String
    /// How it is delivered: "Direct", "Web page", "YouTube", "Peer to peer".
    var kind: String
    /// What Haven's player opens; nil when Haven cannot play the row.
    var address: String?
    /// Who provided it: an add-on's name, or for a media server the add-on it
    /// got the stream from (else the server itself).
    var group: String
    /// The media server item (`srv:…`) the row plays, for a server's row.
    var serverItem: String?
    // Its place under its provider's heading.
    var groupAt = 0
    var shown = true
    /// The provider's heading, on its first row only.
    var heading = ""
    /// How many more the provider has, on its last shown row only.
    var more = 0

    var playable: Bool { address != nil }
}

/// The title page's rules, ported from the Rust build (`library.rs`,
/// `library/sources.rs`, `library/marks.rs`).
enum TitleRules {
    /// The heading over the list of places to watch.
    static let heading = "Play from your sources"
    /// Places to watch shown per provider at first, and added by "Show more".
    static let streamsShown = 10
    /// Names shown under "Cast".
    static let castShown = 6

    #if ADDONS
    static let noDetails = "None of your add-ons has details for this title."
    static let nothingCanPlay = "Nothing you have added can play this. Add a media server or an Xtream Codes account in Settings, or install an add-on that offers streams."
    static let noneHasIt = "None of your media servers, accounts or add-ons has this."
    static let asking = "Asking your media servers and add-ons…"
    #else
    static let noDetails = "There are no details for this title."
    static let nothingCanPlay = "Nothing you have added can play this. Add a media server or an Xtream Codes account in Settings."
    static let noneHasIt = "None of your media servers or accounts has this."
    static let asking = "Asking your media servers…"
    #endif
    static let noTMDBKey = "Add a TMDB key in Settings to see this title's details."
    static let chooseEpisode = "Choose an episode to see where you can watch it."

    // MARK: Details

    /// Year, runtime, rating and genres on one line.
    static func facts(_ meta: MetaDetail) -> String {
        var parts: [String] = []
        if let year = meta.releaseInfo { parts.append(year) }
        if let runtime = meta.runtime { parts.append(runtime) }
        if let rating = meta.rating { parts.append("★ \(rating)") }
        if !meta.genres.isEmpty { parts.append(meta.genres.prefix(3).joined(separator: ", ")) }
        return parts.joined(separator: "  ·  ")
    }

    static func episodeRow(_ video: Video) -> EpisodeRow {
        let title = video.title ?? video.episode.map { "Episode \($0)" } ?? "Untitled"
        return EpisodeRow(id: video.id, number: video.episode.map { "E\($0)" } ?? "", title: title,
                          date: video.released.map { String($0.prefix(10)) } ?? "")
    }

    /// "S1 E2 · Its name", as playing an episode records it.
    static func episodeDetail(_ video: Video) -> String {
        let number = switch (video.season, video.episode) {
        case let (season?, episode?): "S\(season) E\(episode)"
        case let (nil, episode?): "E\(episode)"
        default: ""
        }
        return [number, video.title ?? ""].filter { !$0.isEmpty }.joined(separator: " · ")
    }

    /// Has come out by `today` (an ISO date); no date counts as out.
    static func aired(_ video: Video, today: String) -> Bool {
        guard let day = video.released else { return true }
        return String(day.prefix(10)) <= today
    }

    /// The episode after `current`, in the order they are watched (specials
    /// are not in the run).
    static func nextVideo(_ meta: MetaDetail, after current: String) -> Video? {
        func order(_ video: Video) -> (Int, Int) { (video.season ?? Int.min, video.episode ?? Int.min) }
        var run: [(place: Int, video: Video)] = []
        for (place, video) in meta.videos.enumerated() where video.season.map({ $0 > 0 }) ?? true {
            run.append((place, video))
        }
        run.sort { a, b in order(a.video) != order(b.video) ? order(a.video) < order(b.video) : a.place < b.place }
        guard let at = run.firstIndex(where: { $0.video.id == current }), at + 1 < run.count else { return nil }
        return run[at + 1].video
    }

    // MARK: Who is asked

    /// Describes the film or episode `videoID` of the open title, so media
    /// servers can be asked whether they have it.
    static func wanted(kind: String, videoID: String, openID: String, meta: MetaDetail?, name: String) -> Wanted {
        // The IMDb id: named by the details, or the title's own id, or the
        // start of an episode id such as `tt123:1:2`.
        func imdbOrNil(_ id: String?) -> String? { id.flatMap { TMDB.isIMDbID($0) ? $0 : nil } }
        let episodeStart = videoID.split(separator: ":").first.map(String.init)
        let imdb: String? = meta?.imdbID ?? imdbOrNil(meta?.id) ?? imdbOrNil(openID) ?? imdbOrNil(episodeStart)
        let tmdb: String? = meta?.tmdbID ?? TMDB.splitID(openID).map { String($0.number) }
        var wanted = Wanted(series: kind == "series", name: meta?.name ?? name, year: meta?.releaseInfo, imdb: imdb, tmdb: tmdb)
        if let video = meta?.videos.first(where: { $0.id == videoID }), let season = video.season, let number = video.episode {
            wanted.episode = (season, number)
        }
        return wanted
    }

    /// What add-ons know the film or episode as: its own id if it came from
    /// an add-on, else the IMDb form (`tt…`, `tt…:season:episode`).
    static func addonID(videoID: String, wanted: Wanted) -> String? {
        let foreign = Servers.splitID(videoID) != nil || videoID.hasPrefix("tmdb:")
        guard foreign else { return videoID }
        guard let imdb = wanted.imdb else { return nil }
        if wanted.series {
            guard let (season, number) = wanted.episode else { return nil }
            return "\(imdb):\(season):\(number)"
        }
        return imdb
    }

    /// The IMDb id of the title in `found` (a catalogue search) with the
    /// wanted name and, when both are known, year.
    static func imdbMatch(_ found: [MetaPreview], name: String, year: String?) -> String? {
        func plain(_ text: String) -> String { String(text.filter { $0.isLetter || $0.isNumber }).lowercased() }
        func yearOf(_ text: String?) -> Int? { text.flatMap { $0.count >= 4 ? Int($0.prefix(4)) : nil } }
        let wantedName = plain(name)
        guard !wantedName.isEmpty else { return nil }
        return found.first { title in
            guard TMDB.isIMDbID(title.id), plain(title.name) == wantedName else { return false }
            if let a = yearOf(year), let b = yearOf(title.releaseInfo) { return a == b }
            return true
        }?.id
    }

    // MARK: Rows

    /// A row for one stream, or nil if it names no source.
    static func streamRow(id: Int, source: String, _ stream: MediaStream) -> StreamRow? {
        guard let kind = stream.kind else { return nil }
        let label = switch kind {
        case .direct: "Direct"
        case .external: "Web page"
        case .youTube: "YouTube"
        case .peerToPeer: "Peer to peer"
        }
        func oneLine(_ text: String) -> String { text.split(whereSeparator: \.isWhitespace).joined(separator: " ") }
        // The detail goes on two lines: the release's name, then its facts.
        // The size and how many share it are taken out and shown on their own.
        var detail = oneLine(stream.details ?? "")
        var size = stream.videoSize.map(sizeText)
        if let (text, range) = sizeIn(detail) {
            size = size ?? text
            detail = tidy(cut(detail, range))
        }
        var peers = ""
        if let (count, range) = peersIn(detail) {
            peers = count
            detail = tidy(cut(detail, range))
        }
        let parts = detail.components(separatedBy: " · ")
        var first = ""
        var used = 0
        for part in parts {
            if used > 0 && first.count + part.count > 46 { break }
            if used > 0 { first += " · " }
            first += part
            used += 1
        }
        let address = kind == .direct ? stream.url.flatMap(PlayableAddress.web) : nil
        return StreamRow(id: id, source: source, name: oneLine(stream.name ?? "Stream"), detail: first,
                         detail2: parts[used...].joined(separator: " · "), size: size ?? "", peers: peers, kind: label,
                         address: address, group: stream.provider ?? source)
    }

    private static func cut(_ text: String, _ range: Range<Int>) -> String {
        let chars = Array(text)
        return String(chars[..<range.lowerBound]) + String(chars[range.upperBound...])
    }

    /// A size in bytes as people read it: "12.3 GB", "850 MB".
    static func sizeText(_ bytes: Int64) -> String {
        let gb = Double(bytes) / 1_073_741_824
        return gb >= 1 ? String(format: "%.1f GB", gb) : String(format: "%.0f MB", Double(bytes) / 1_048_576)
    }

    /// The first size written in a text ("12.3 GB", "850MB", "1,4 GiB") as
    /// Haven shows it, and where it is (in characters; a 💾 before it counts
    /// as part of it).
    static func sizeIn(_ text: String) -> (String, Range<Int>)? {
        let units: [(written: String, shown: String)] = [("TIB", "TB"), ("TB", "TB"), ("GIB", "GB"), ("GB", "GB"), ("MIB", "MB"), ("MB", "MB")]
        let chars = Array(text)
        func digit(_ at: Int) -> Bool { at < chars.count && chars[at].isASCII && chars[at].isNumber }
        func alnum(_ at: Int) -> Bool { chars[at].isASCII && (chars[at].isLetter || chars[at].isNumber) }
        var at = 0
        while at < chars.count {
            guard digit(at), at == 0 || !alnum(at - 1) else {
                at += 1
                continue
            }
            var end = at
            while digit(end) || ((chars[safe: end] == "." || chars[safe: end] == ",") && digit(end + 1)) { end += 1 }
            let number = String(chars[at ..< end]).replacingOccurrences(of: ",", with: ".")
            var unitAt = end
            while chars[safe: unitAt] == " " { unitAt += 1 }
            let rest = Array(String(chars[unitAt...]).uppercased())
            if let unit = units.first(where: { unit in
                let written = Array(unit.written)
                guard rest.starts(with: written) else { return false }
                return !(rest[safe: written.count].map { $0.isASCII && $0.isLetter } ?? false)
            }) {
                var start = at
                let before = String(chars[..<at]).replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression)
                if before.hasSuffix("💾") { start = before.count - 1 }
                return ("\(number) \(unit.shown)", start ..< (unitAt + unit.written.count))
            }
            at = max(end, at + 1)
        }
        return nil
    }

    /// How many share the file, where a detail line names it with a people
    /// sign ("👥 418", "👤 50"), and where that is (in characters).
    static func peersIn(_ text: String) -> (String, Range<Int>)? {
        let chars = Array(text)
        for mark in ["\u{1F465}", "\u{1F464}"] {
            guard let at = chars.firstIndex(where: { $0.unicodeScalars.first.map { String($0) } == mark }) else { continue }
            var from = at + 1
            while let char = chars[safe: from], char == " " || char == ":" || char == "\u{FE0F}" { from += 1 }
            var end = from
            while let char = chars[safe: end], (char.isASCII && char.isNumber) || char == "," { end += 1 }
            let count = String(chars[from ..< end]).replacingOccurrences(of: ",", with: "")
            if !count.isEmpty { return (count, at ..< end) }
        }
        return nil
    }

    /// Cleans up a detail line once something was taken out of it: no
    /// doubled spaces or separators, none left at either end.
    static func tidy(_ text: String) -> String {
        let joined = text.split(whereSeparator: \.isWhitespace).joined(separator: " ")
        let parts = joined.components(separatedBy: " · ")
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty && $0 != "·" }
        return parts.joined(separator: " · ").trimmingCharacters(in: CharacterSet(charactersIn: "·|").union(.whitespaces))
    }

    /// Where a new row goes: your own servers stay above the add-ons, and
    /// each provider's rows stay together.
    static func place(_ row: StreamRow, in rows: [StreamRow], serverRows: Int, fromServer: Bool) -> Int {
        let (from, to) = fromServer ? (0, serverRows) : (serverRows, rows.count)
        guard from < to, let last = (from ..< to).reversed().first(where: { rows[$0].group == row.group }) else { return to }
        return last + 1
    }

    /// Each row's place under its provider's heading: which rows show (ten
    /// per provider, more on asking), which carries the heading and which
    /// the "Show more".
    static func marked(_ rows: [StreamRow], limits: [String: Int]) -> [StreamRow] {
        var counts: [String: Int] = [:]
        for row in rows { counts[row.group, default: 0] += 1 }
        var places: [String: Int] = [:]
        return rows.map { row in
            var row = row
            let place = places[row.group, default: 0]
            let count = counts[row.group] ?? 0
            let limit = limits[row.group] ?? streamsShown
            row.groupAt = place
            row.shown = place < limit
            row.heading = place == 0 ? "\(row.group)  ·  \(count)" : ""
            row.more = place == limit - 1 && count > limit ? count - limit : 0
            places[row.group] = place + 1
            return row
        }
    }

    /// The providers in the order they first appear, each with how many rows.
    static func groups(_ rows: [StreamRow]) -> [(name: String, count: Int)] {
        var groups: [(name: String, count: Int)] = []
        for row in rows {
            if let at = groups.firstIndex(where: { $0.name == row.group }) {
                groups[at].count += 1
            } else {
                groups.append((row.group, 1))
            }
        }
        return groups
    }
}

extension Array {
    subscript(safe index: Int) -> Element? { indices.contains(index) ? self[index] : nil }
}
