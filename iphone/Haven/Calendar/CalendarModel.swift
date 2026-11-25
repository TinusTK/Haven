import Foundation
import Observation

/// The Calendar page: what comes out from the Watchlist, a month or a week
/// at a time. On a phone the day picked lists its releases. The Rust build's
/// `library/calendar.rs` (`show_calendar`, `step_calendar`).
enum CalendarRules {
    static let weekdays = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
    static let weekdayNames = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"]

    /// One film or episode on a day.
    struct Entry: Equatable, Sendable, Identifiable {
        var kind: String
        var id: String
        var title: String
        /// "S3 E5 · Its name"; empty for a film.
        var label: String
        /// The episode's id; empty for a film.
        var video: String
        /// "21:00"; empty when only the date is known.
        var time: String
        var watched: Bool

        /// The second line: the episode, "Film" or "Comes out".
        var line: String { !label.isEmpty ? label : kind == "movie" ? "Film" : "Comes out" }
    }

    /// One square of the page.
    struct Square: Equatable, Sendable, Identifiable {
        var day: Day
        var inMonth: Bool
        var today: Bool
        var entries: [Entry]
        /// "Wednesday 25 November"
        var longName: String
        var id: Day { day }
        /// Every release shown has been watched (a tick instead of dots).
        var allWatched: Bool { !entries.isEmpty && entries.allSatisfy(\.watched) }
    }

    struct Page: Equatable, Sendable {
        /// "November 2026", "23 – 29 November 2026", "30 Nov – 6 Dec 2026"
        var title: String
        var squares: [Square]
        var note: String
    }

    /// The page for the month or week holding `anchor`. `watched` gives a
    /// title's finished films and episodes.
    static func page(releases: [Releases.Title: [Release]], names: [Releases.Title: String],
                     watched: (Releases.Title) -> Set<String>, anchor: Day, week: Bool, today: Day, waiting: Bool) -> Page {
        var byDay: [Day: [(time: Clock?, entry: Entry)]] = [:]
        for (title, list) in releases {
            guard let name = names[title] else { continue }
            let done = watched(title)
            for release in list {
                let seen = release.video.isEmpty ? !done.isEmpty : done.contains(release.video)
                byDay[release.day, default: []].append((release.time, Entry(
                    kind: title.kind, id: title.id, title: name, label: release.label, video: release.video,
                    time: release.time?.text ?? "", watched: seen)))
            }
        }
        // In order of time (those with none last), then name.
        func before(_ a: (time: Clock?, entry: Entry), _ b: (time: Clock?, entry: Entry)) -> Bool {
            switch (a.time, b.time) {
            case let (x?, y?) where x != y: return x < y
            case (nil, _?): return false
            case (_?, nil): return true
            default: return (a.entry.title, a.entry.label) < (b.entry.title, b.entry.label)
            }
        }
        let days = week ? Releases.week(of: anchor) : Releases.month(year: anchor.year, month: anchor.month)
        let squares = days.map { day in
            Square(day: day, inMonth: week || day.month == anchor.month, today: day == today,
                   entries: (byDay[day] ?? []).sorted(by: before).map(\.entry),
                   longName: "\(weekdayNames[day.weekdayFromMonday]) \(day.day) \(Releases.months[day.month - 1])")
        }
        let shown = squares.filter(\.inMonth).flatMap(\.entries)
        let note = if names.isEmpty {
            "Add films and series to your Watchlist to see here when they come out."
        } else if waiting {
            "Looking up dates…"
        } else if shown.isEmpty {
            week ? "Nothing on your Watchlist comes out this week." : "Nothing on your Watchlist comes out this month."
        } else {
            ""
        }
        return Page(title: title(days: days, anchor: anchor, week: week), squares: squares, note: note)
    }

    static func title(days: [Day], anchor: Day, week: Bool) -> String {
        guard week, let first = days.first, let last = days.last else {
            return "\(Releases.months[anchor.month - 1]) \(anchor.year)"
        }
        if first.month == last.month {
            return "\(first.day) – \(last.day) \(Releases.months[last.month - 1]) \(last.year)"
        }
        return "\(first.day) \(Releases.months[first.month - 1].prefix(3)) – \(last.day) \(Releases.months[last.month - 1].prefix(3)) \(last.year)"
    }

    /// A month (or week) back (-1) or on (1), or today (0).
    static func step(_ anchor: Day, by step: Int, week: Bool, today: Day) -> Day {
        if step == 0 { return today }
        if week { return anchor.adding(days: step < 0 ? -7 : 7) }
        var month = anchor.month + (step < 0 ? -1 : 1)
        var year = anchor.year
        if month < 1 { month = 12; year -= 1 }
        if month > 12 { month = 1; year += 1 }
        return Day(year: year, month: month, day: 1)
    }

    /// "Times are in your local time (UTC+11:00)."
    static func zoneNote(_ zone: TimeZone, at date: Date = Date()) -> String {
        let seconds = zone.secondsFromGMT(for: date)
        let sign = seconds < 0 ? "-" : "+"
        let minutes = abs(seconds) / 60
        return String(format: "Times are in your local time (UTC%@%02d:%02d).", sign, minutes / 60, minutes % 60)
    }

    /// The day to list after the page changed: the one picked while it is
    /// still on the page, else today, else the first day of the month.
    static func pick(_ before: Day?, in squares: [Square], today: Day) -> Day? {
        if let before, squares.contains(where: { $0.day == before }) { return before }
        if squares.contains(where: { $0.day == today }) { return today }
        return squares.first(where: \.inMonth)?.day ?? squares.first?.day
    }
}

/// What the Calendar page has chosen: the month or week showing and the day picked.
@MainActor @Observable
final class CalendarModel {
    @ObservationIgnored private let library: Library
    /// A day in the month or week showing.
    private(set) var anchor: Day
    /// A week at a time instead of a month.
    var week = false { didSet { picked = nil } }
    /// The day whose releases are listed (nil: today, or the month's first).
    var picked: Day?

    init(library: Library) {
        self.library = library
        anchor = Day.today(library.zone)
    }

    var today: Day { Day.today(library.zone) }

    var page: CalendarRules.Page {
        let names = Dictionary(library.watchlist.map { (Releases.Title(kind: $0.kind, id: $0.titleID), $0.title) },
                               uniquingKeysWith: { first, _ in first })
        _ = library.marks
        let store = library.store
        return CalendarRules.page(releases: library.allReleases, names: names,
                                  watched: { Set((try? store.finishedVideos(kind: $0.kind, titleID: $0.id)) ?? []) },
                                  anchor: anchor, week: week, today: today, waiting: library.calendarWaiting > 0)
    }

    /// The square listed below the page.
    func pickedSquare(_ page: CalendarRules.Page) -> CalendarRules.Square? {
        let day = CalendarRules.pick(picked, in: page.squares, today: today)
        return page.squares.first { $0.day == day }
    }

    func step(_ by: Int) {
        anchor = CalendarRules.step(anchor, by: by, week: week, today: today)
        if by == 0 { picked = today }
    }

    /// The page opened: dates of titles not asked for yet are looked up.
    func appeared() {
        library.refreshCalendar()
    }
}
