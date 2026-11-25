import Foundation
import Testing
@testable import Haven

/// The Calendar page (the Rust build's `show_calendar` and `step_calendar`).
struct CalendarPageTests {
    private let show = Releases.Title(kind: "series", id: "tt1")
    private let film = Releases.Title(kind: "movie", id: "tt2")
    private let gone = Releases.Title(kind: "movie", id: "tt3")
    private let today = Day(year: 2026, month: 11, day: 25)

    private func release(_ day: Int, _ time: Clock?, _ label: String, _ video: String, month: Int = 11) -> Release {
        Release(day: Day(year: 2026, month: month, day: day), time: time, label: label, video: video, number: "", name: "")
    }

    private func page(week: Bool = false, anchor: Day? = nil, watched: [Releases.Title: Set<String>] = [:], waiting: Bool = false,
                      names: [Releases.Title: String]? = nil) -> CalendarRules.Page {
        CalendarRules.page(
            releases: [
                show: [release(25, Clock(hour: 21, minute: 0), "S1 E2 · Two", "tt1:1:2"),
                       release(25, nil, "S1 E3 · Three", "tt1:1:3"),
                       release(2, Clock(hour: 8, minute: 0), "S1 E4 · Four", "tt1:1:4", month: 12)],
                film: [release(25, Clock(hour: 9, minute: 30), "", "")],
                gone: [release(25, nil, "", "")],
            ],
            names: names ?? [show: "A Show", film: "A Film"], watched: { watched[$0] ?? [] },
            anchor: anchor ?? today, week: week, today: today, waiting: waiting)
    }

    @Test func aDayListsItsReleasesByTimeThenName() throws {
        let page = page(watched: [show: ["tt1:1:2"], film: ["tt2"]])
        #expect(page.title == "November 2026" && page.squares.count == 42 && page.note.isEmpty)
        let day = try #require(page.squares.first { $0.day == today })
        #expect(day.today && day.inMonth && day.longName == "Wednesday 25 November")
        #expect(day.entries.map(\.title) == ["A Film", "A Show", "A Show"], "a title off the Watchlist is left out")
        #expect(day.entries.map(\.time) == ["09:30", "21:00", ""])
        #expect(day.entries.map(\.watched) == [true, true, false])
        #expect(day.entries.map(\.line) == ["Film", "S1 E2 · Two", "S1 E3 · Three"])
        #expect(!day.allWatched)
        // December's days on the page are dimmed but still show their releases.
        let december = try #require(page.squares.first { $0.day == Day(year: 2026, month: 12, day: 2) })
        #expect(!december.inMonth && december.entries.count == 1)
    }

    @Test func aWeekShowsSevenDays() {
        let page = page(week: true)
        #expect(page.squares.count == 7 && page.squares.allSatisfy(\.inMonth))
        #expect(page.title == "23 – 29 November 2026")
        let turning = self.page(week: true, anchor: Day(year: 2026, month: 12, day: 2))
        #expect(turning.title == "30 Nov – 6 Dec 2026")
        #expect(self.page(week: true, anchor: Day(year: 2026, month: 12, day: 9)).note == "Nothing on your Watchlist comes out this week.")
    }

    @Test func theNoteSaysWhyNothingShows() {
        #expect(page(names: [:]).note == "Add films and series to your Watchlist to see here when they come out.")
        #expect(page(waiting: true).note == "Looking up dates…")
        #expect(page(anchor: Day(year: 2027, month: 3, day: 1)).note == "Nothing on your Watchlist comes out this month.")
    }

    @Test func stepsGoAMonthOrAWeek() {
        #expect(CalendarRules.step(Day(year: 2026, month: 12, day: 15), by: 1, week: false, today: today) == Day(year: 2027, month: 1, day: 1))
        #expect(CalendarRules.step(Day(year: 2026, month: 1, day: 15), by: -1, week: false, today: today) == Day(year: 2025, month: 12, day: 1))
        #expect(CalendarRules.step(today, by: 1, week: true, today: today) == Day(year: 2026, month: 12, day: 2))
        #expect(CalendarRules.step(Day(year: 2020, month: 1, day: 1), by: 0, week: false, today: today) == today)
    }

    @Test func theDayPickedStaysWhileItIsOnThePage() {
        let squares = page().squares
        #expect(CalendarRules.pick(Day(year: 2026, month: 11, day: 3), in: squares, today: today) == Day(year: 2026, month: 11, day: 3))
        #expect(CalendarRules.pick(nil, in: squares, today: today) == today)
        let march = page(anchor: Day(year: 2027, month: 3, day: 1)).squares
        #expect(CalendarRules.pick(today, in: march, today: today) == Day(year: 2027, month: 3, day: 1))
    }

    @Test func theTimeZoneIsNamed() {
        #expect(CalendarRules.zoneNote(TimeZone(secondsFromGMT: 11 * 3600)!) == "Times are in your local time (UTC+11:00).")
        #expect(CalendarRules.zoneNote(TimeZone(secondsFromGMT: -(5 * 3600 + 1800))!) == "Times are in your local time (UTC-05:30).")
    }
}
