import Foundation

/// A calendar day, without a time zone.
struct Day: Hashable, Comparable, Sendable, CustomStringConvertible {
    var year: Int
    var month: Int
    var day: Int

    static func < (a: Day, b: Day) -> Bool {
        (a.year, a.month, a.day) < (b.year, b.month, b.day)
    }

    /// "2026-11-25".
    var description: String { String(format: "%04d-%02d-%02d", year, month, day) }

    /// Reads "2026-11-25" (the first ten characters of a longer text).
    init?(_ text: String) {
        let head = text.prefix(10).split(separator: "-")
        guard head.count == 3, head[0].count == 4, let y = Int(head[0]), let m = Int(head[1]), let d = Int(head[2]),
              (1 ... 12).contains(m), (1 ... 31).contains(d) else { return nil }
        self.init(year: y, month: m, day: d)
        guard date(in: TimeZone(identifier: "UTC")!) != nil else { return nil }
    }

    init(year: Int, month: Int, day: Int) {
        self.year = year
        self.month = month
        self.day = day
    }

    /// Today where the phone is.
    static func today(_ zone: TimeZone = .current) -> Day { Day(Date(), zone) }

    init(_ date: Date, _ zone: TimeZone) {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = zone
        let parts = calendar.dateComponents([.year, .month, .day], from: date)
        self.init(year: parts.year ?? 1970, month: parts.month ?? 1, day: parts.day ?? 1)
    }

    /// Midday of this day in `zone` (for counting days apart).
    func date(in zone: TimeZone) -> Date? {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = zone
        let made = calendar.date(from: DateComponents(year: year, month: month, day: day, hour: 12))
        guard let made, Day(made, zone) == self else { return nil }
        return made
    }

    /// The day `count` days on (back when negative).
    func adding(days count: Int) -> Day {
        let utc = TimeZone(identifier: "UTC")!
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = utc
        guard let start = date(in: utc), let moved = calendar.date(byAdding: .day, value: count, to: start) else { return self }
        return Day(moved, utc)
    }

    /// Days from `self` to `other`.
    func days(to other: Day) -> Int {
        let utc = TimeZone(identifier: "UTC")!
        guard let a = date(in: utc), let b = other.date(in: utc) else { return 0 }
        return Int((b.timeIntervalSince(a) / 86_400).rounded())
    }

    /// 0 for Monday … 6 for Sunday.
    var weekdayFromMonday: Int {
        let utc = TimeZone(identifier: "UTC")!
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = utc
        guard let date = date(in: utc) else { return 0 }
        return (calendar.component(.weekday, from: date) + 5) % 7
    }
}

/// A time of day, in the phone's time zone.
struct Clock: Hashable, Comparable, Sendable {
    var hour: Int
    var minute: Int

    static func < (a: Clock, b: Clock) -> Bool { (a.hour, a.minute) < (b.hour, b.minute) }

    /// "21:00".
    var text: String { String(format: "%02d:%02d", hour, minute) }
}

/// One film or episode coming out.
struct Release: Equatable, Sendable {
    var day: Day
    /// The local time of day, when the source gives a time.
    var time: Clock?
    /// "S3 E5 · Its name" for an episode; empty for a film.
    var label: String
    /// The episode's own id, to open it by; empty for a film.
    var video: String
    /// "S03E05" for an episode; empty for a film.
    var number: String
    /// The episode's name, if it has one.
    var name: String
}

/// When things on the Watchlist come out (the Rust build's calendar rules).
enum Releases {
    static let months = ["January", "February", "March", "April", "May", "June", "July", "August",
                         "September", "October", "November", "December"]

    /// When something released at `released` (an ISO date, or a moment with
    /// a time zone) comes out in `zone`. A moment is moved to that zone, so
    /// an evening in America is the next day in Australia. A bare date, or
    /// midnight UTC (how sources write a date they have no time for), stays
    /// that date: there is no time to move.
    static func local(_ released: String, zone: TimeZone) -> (day: Day, time: Clock?)? {
        let released = released.trimmingCharacters(in: .whitespacesAndNewlines)
        if released.count > 10, let moment = moment(released) {
            let utc = TimeZone(identifier: "UTC")!
            var calendar = Calendar(identifier: .gregorian)
            calendar.timeZone = utc
            let inUTC = calendar.dateComponents([.hour, .minute, .second, .nanosecond], from: moment)
            if inUTC.hour == 0 && inUTC.minute == 0 && inUTC.second == 0 && (inUTC.nanosecond ?? 0) < 1_000_000 {
                return (Day(moment, utc), nil)
            }
            calendar.timeZone = zone
            let local = calendar.dateComponents([.hour, .minute], from: moment)
            return (Day(moment, zone), Clock(hour: local.hour ?? 0, minute: local.minute ?? 0))
        }
        guard let day = Day(released) else { return nil }
        return (day, nil)
    }

    /// An RFC 3339 moment, with or without fractions of a second.
    static func moment(_ text: String) -> Date? {
        let withFraction = ISO8601DateFormatter()
        withFraction.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return withFraction.date(from: text) ?? ISO8601DateFormatter().date(from: text)
    }

    /// What a title's details say comes out, and when, in `zone`.
    static func of(_ meta: MetaDetail, zone: TimeZone) -> [Release] {
        var releases: [Release] = meta.videos
            // Specials (season 0) are left out: sources list making-of pieces
            // there on the same day as the real episodes.
            .filter { $0.season != 0 }
            .compactMap { video in
                guard let released = video.released, let (day, time) = local(released, zone: zone) else { return nil }
                let number: String
                let short: String
                switch (video.season, video.episode) {
                case let (season?, episode?):
                    number = "S\(season) E\(episode)"
                    short = String(format: "S%02dE%02d", season, episode)
                case let (nil, episode?):
                    number = "E\(episode)"
                    short = String(format: "E%02d", episode)
                default:
                    number = ""
                    short = ""
                }
                let label = [number, video.title ?? ""].filter { !$0.isEmpty }.joined(separator: " · ")
                return Release(day: day, time: time, label: label, video: video.id, number: short, name: video.title ?? "")
            }
        // The same episode listed twice (some sources repeat it) shows once.
        var seen = Set<String>()
        releases = releases.filter { seen.insert("\($0.day)|\($0.label)").inserted }
        // A film, or a series with no episodes listed: the title's own date.
        if releases.isEmpty, let released = meta.released, let (day, time) = local(released, zone: zone) {
            releases.append(Release(day: day, time: time, label: "", video: "", number: "", name: ""))
        }
        return releases
    }

    /// The 7 days of the week (Monday first) that holds `day`.
    static func week(of day: Day) -> [Day] {
        let start = day.adding(days: -day.weekdayFromMonday)
        return (0 ..< 7).map { start.adding(days: $0) }
    }

    /// The 42 days a month's page shows: whole weeks from the Monday on or
    /// before the 1st.
    static func month(year: Int, month: Int) -> [Day] {
        let first = Day(year: year, month: month, day: 1)
        let start = first.adding(days: -first.weekdayFromMonday)
        return (0 ..< 42).map { start.adding(days: $0) }
    }

    /// A title on the Watchlist: its kind and id.
    struct Title: Hashable, Sendable {
        var kind: String
        var id: String
    }

    /// One card in Home's Upcoming row.
    struct Card: Equatable, Sendable, Identifiable {
        var id: String
        var kind: String
        /// The episode to open; empty for a film.
        var video: String
        var title: String
        /// The episode's name, "S03E05", "New episodes" or "Film".
        var episode: String
        /// "S03E05", or "S03E07–E08" when more come the same day.
        var badge: String
        /// "Today", "Tomorrow", "In 4 days", "Nov 5" (as built; on show,
        /// `when(at:)` keeps it up to date).
        var when: String
        /// Less than a week away: the pill is gold.
        var soon: Bool
        /// The title's wide picture (else its poster).
        var picture: String?
        /// When it comes out (local).
        var day: Day
        var time: Clock?

        /// The label at `now`: on the day it counts down by the hour.
        func when(at now: Date, zone: TimeZone = .current) -> String {
            Releases.whenLabel(day: day, time: time, now: now, zone: zone)
        }
    }

    /// How soon something out on `day` (at `time`, when known) comes out,
    /// seen at `now`: "Nov 5", "In 6 days", "Tomorrow", and on the day
    /// "In 5 h", "In 40 min", "Out now" (or "Today" with no time).
    static func whenLabel(day: Day, time: Clock?, now: Date, zone: TimeZone = .current) -> String {
        let days = Day(now, zone).days(to: day)
        switch days {
        case ...0:
            guard let time else { return "Today" }
            var calendar = Calendar(identifier: .gregorian)
            calendar.timeZone = zone
            guard let out = calendar.date(from: DateComponents(year: day.year, month: day.month, day: day.day,
                                                               hour: time.hour, minute: time.minute)) else { return "Today" }
            let minutes = Int(out.timeIntervalSince(now) / 60)
            if minutes <= 0 { return "Out now" }
            if minutes < 60 { return "In \(minutes) min" }
            // Whole hours left: 4 h 50 min reads "In 4 h".
            return "In \(minutes / 60) h"
        case 1: return "Tomorrow"
        case 2 ... 6: return "In \(days) days"
        default: return "\(months[day.month - 1].prefix(3)) \(day.day)"
        }
    }

    /// Home's Upcoming row: from today on, one card a show (its next
    /// release; more of it the same day widen the badge), at most 12.
    static func upcoming(_ releases: [Title: [Release]], names: [Title: String], pictures: [Title: String], today: Day) -> [Card] {
        var coming: [(title: Title, release: Release)] = releases
            .flatMap { title, list in list.map { (title, $0) } }
            .filter { $0.release.day >= today }
        coming.sort { a, b in
            if a.release.day != b.release.day { return a.release.day < b.release.day }
            // A release with no time comes before one with a time (Rust's None < Some).
            switch (a.release.time, b.release.time) {
            case let (x?, y?) where x != y: return x < y
            case (nil, _?): return true
            case (_?, nil): return false
            default: return a.title.id < b.title.id
            }
        }
        // The last release of each show on its first day.
        var firstDay: [Title: Day] = [:]
        var lastSameDay: [Title: Release] = [:]
        for (title, release) in coming {
            if firstDay[title] == nil { firstDay[title] = release.day }
            if firstDay[title] == release.day { lastSameDay[title] = release }
        }
        var seen = Set<Title>()
        coming = coming.filter { seen.insert($0.title).inserted }
        return coming.compactMap { title, release -> Card? in
            guard let name = names[title] else { return nil }
            let last = lastSameDay[title].flatMap { $0.number != release.number ? $0 : nil }
            let days = today.days(to: release.day)
            let when: String
            switch days {
            case 0: when = release.time == nil ? "Today" : whenLabel(day: release.day, time: release.time, now: Date())
            case 1: when = "Tomorrow"
            case 2 ... 6: when = "In \(days) days"
            default: when = "\(months[release.day.month - 1].prefix(3)) \(release.day.day)"
            }
            let episode = last != nil ? "New episodes"
                : !release.name.isEmpty ? release.name
                : release.number.isEmpty ? "Film" : release.number
            let badge: String
            if let last, last.number.count > 3 {
                badge = "\(release.number)–\(last.number.suffix(3))"
            } else {
                badge = release.number
            }
            return Card(id: title.id, kind: title.kind, video: release.video, title: name, episode: episode,
                        badge: badge, when: when, soon: days < 7, picture: pictures[title],
                        day: release.day, time: release.time)
        }
        .prefix(12)
        .map { $0 }
    }
}
