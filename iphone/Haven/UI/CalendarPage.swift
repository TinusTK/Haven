import SwiftUI

/// The Calendar: a month (or week) of squares, a dot for each film or
/// episode from the Watchlist coming out (a tick once watched), and the
/// picked day's releases under them.
struct CalendarPage: View {
    let model: AppModel
    let calendar: CalendarModel

    var body: some View {
        let page = calendar.page
        let picked = calendar.pickedSquare(page)
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 12) {
                    PageTitle(text: "Calendar")
                    Spacer(minLength: 0)
                    Segments(labels: ["Month", "Week"], current: Binding(get: { calendar.week ? 1 : 0 }, set: { calendar.week = $0 == 1 }))
                        .fixedSize()
                }
                HStack(spacing: 8) {
                    step("back", name: calendar.week ? "The week before" : "The month before") { calendar.step(-1) }
                    Text(page.title)
                        .font(Palette.font(15, .bold))
                        .foregroundStyle(Palette.text)
                        .frame(maxWidth: .infinity)
                        .lineLimit(1)
                        .minimumScaleFactor(0.8)
                    step("forward", name: calendar.week ? "The week after" : "The month after") { calendar.step(1) }
                    Chip(label: "Today") { calendar.step(0) }
                }
                Grid(page: page, picked: picked?.day) { calendar.picked = $0 }
                if !page.note.isEmpty {
                    Text(page.note).font(Palette.font(13)).foregroundStyle(Palette.textDim)
                }
                if let picked {
                    Text(picked.longName).font(Palette.heading(18)).foregroundStyle(Palette.text).padding(.top, 4)
                    if picked.entries.isEmpty {
                        Text("Nothing from your Watchlist comes out this day.")
                            .font(Palette.font(14))
                            .foregroundStyle(Palette.textDim)
                    }
                    ForEach(Array(picked.entries.enumerated()), id: \.offset) { _, entry in
                        EntryRow(entry: entry) {
                            model.open(Opening(id: entry.id, kind: entry.kind, title: entry.title,
                                               video: entry.video.isEmpty ? nil : entry.video))
                        }
                    }
                }
                Text(CalendarRules.zoneNote(model.library.zone))
                    .font(Palette.font(11))
                    .foregroundStyle(Palette.textDim)
                    .padding(.top, 6)
            }
            .padding(.horizontal, 16)
            .padding(.top, 12)
            .padding(.bottom, 24)
        }
        .onAppear { calendar.appeared() }
    }

    private func step(_ icon: String, name: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HavenIcon(name: icon, size: 18, tint: Palette.text)
                .frame(width: 36, height: 36)
                .background(Circle().fill(Palette.surface))
                .overlay(Circle().strokeBorder(Palette.border, lineWidth: 1))
        }
        .buttonStyle(PressedStyle())
        .accessibilityLabel(name)
    }
}

/// The squares, seven across under the days' names.
private struct Grid: View {
    let page: CalendarRules.Page
    let picked: Day?
    let pick: (Day) -> Void

    var body: some View {
        let columns = Array(repeating: GridItem(.flexible(), spacing: 6), count: 7)
        VStack(spacing: 6) {
            LazyVGrid(columns: columns, spacing: 6) {
                ForEach(CalendarRules.weekdays, id: \.self) { name in
                    Text(name).font(Palette.font(11, .semibold)).foregroundStyle(Palette.textDim)
                }
            }
            .accessibilityHidden(true)
            LazyVGrid(columns: columns, spacing: 6) {
                ForEach(page.squares) { square in
                    DaySquare(square: square, picked: square.day == picked) { pick(square.day) }
                }
            }
        }
    }
}

private struct DaySquare: View {
    let square: CalendarRules.Square
    let picked: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: 4) {
                Text("\(square.day.day)")
                    .font(Palette.font(14, square.today || picked ? .bold : .medium))
                    .foregroundStyle(square.today || picked ? Palette.accent : square.inMonth ? Palette.text : Palette.textDim)
                    .opacity(square.inMonth ? 1 : 0.5)
                Group {
                    if square.allWatched {
                        WatchedTick(size: 16)
                    } else if !square.entries.isEmpty {
                        // One dot a release, up to three.
                        HStack(spacing: 3) {
                            ForEach(0 ..< min(square.entries.count, 3), id: \.self) { _ in
                                Circle().fill(Palette.accent).frame(width: 6, height: 6)
                            }
                        }
                        .opacity(square.inMonth ? 1 : 0.55)
                    } else {
                        Color.clear
                    }
                }
                .frame(height: 16)
            }
            .frame(maxWidth: .infinity, minHeight: 50)
            .background(RoundedRectangle(cornerRadius: 10).fill(
                picked ? Palette.accent.opacity(0.22) : square.today ? Palette.selected : square.inMonth ? Palette.surface : Color.clear))
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(
                picked || square.today ? Palette.accent : square.inMonth ? Palette.border : Color.clear,
                lineWidth: picked || square.today ? 1.5 : 1))
        }
        .buttonStyle(PressedStyle())
        .accessibilityLabel(square.longName + (square.entries.isEmpty ? "" : ", \(square.entries.count) coming out"))
        .accessibilityAddTraits(picked ? .isSelected : [])
    }
}

/// One film or episode coming out that day.
private struct EntryRow: View {
    let entry: CalendarRules.Entry
    let open: () -> Void

    var body: some View {
        Button(action: open) {
            HStack(spacing: 12) {
                RoundedRectangle(cornerRadius: 2).fill(Palette.accent).frame(width: 4, height: 30)
                VStack(alignment: .leading, spacing: 2) {
                    Text(entry.title).font(Palette.font(14, .semibold)).foregroundStyle(Palette.text).lineLimit(1)
                    Text(entry.line).font(Palette.font(12)).foregroundStyle(Palette.textDim).lineLimit(1)
                }
                Spacer(minLength: 0)
                if !entry.time.isEmpty {
                    Text(entry.time).font(Palette.font(13, .semibold)).foregroundStyle(Palette.accent)
                }
                if entry.watched { WatchedTick(size: 24) }
            }
            .padding(.horizontal, 12)
            .frame(minHeight: 56)
            .background(RoundedRectangle(cornerRadius: 10).fill(Palette.surface))
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Palette.border, lineWidth: 1))
        }
        .buttonStyle(PressedStyle())
        .accessibilityLabel(entry.title + (entry.label.isEmpty ? "" : ", \(entry.label)") + (entry.watched ? ", watched" : ""))
    }
}
