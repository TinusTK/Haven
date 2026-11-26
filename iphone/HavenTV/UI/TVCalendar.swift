import SwiftUI

/// The Calendar on the television: the month (or week) of squares on the
/// left, a dot for each Watchlist film or episode coming out (a tick once
/// watched), and the picked day's releases on the right.
struct TVCalendarPage: View {
    let model: AppModel
    let calendar: CalendarModel

    var body: some View {
        let page = calendar.page
        let picked = calendar.pickedSquare(page)
        HStack(alignment: .top, spacing: 60) {
            VStack(alignment: .leading, spacing: 24) {
                HStack(spacing: 20) {
                    TVChip(label: "Month", selected: !calendar.week) { calendar.week = false }
                    TVChip(label: "Week", selected: calendar.week) { calendar.week = true }
                    Spacer(minLength: 30)
                    TVChip(label: "", icon: "back") { calendar.step(-1) }
                        .accessibilityLabel(calendar.week ? "The week before" : "The month before")
                    Text(page.title)
                        .font(Palette.font(30, .bold))
                        .foregroundStyle(Palette.text)
                        .lineLimit(1)
                        .minimumScaleFactor(0.7)
                        .frame(minWidth: 320)
                    TVChip(label: "", icon: "forward") { calendar.step(1) }
                        .accessibilityLabel(calendar.week ? "The week after" : "The month after")
                    TVChip(label: "Today") { calendar.step(0) }
                }
                .padding(.vertical, 10)
                .focusSection()
                TVDayGrid(page: page, picked: picked?.day) { calendar.picked = $0 }
                    .focusSection()
                if !page.note.isEmpty { TVNote(text: page.note) }
                Text(CalendarRules.zoneNote(model.library.zone))
                    .font(Palette.font(20))
                    .foregroundStyle(Palette.textDim)
            }
            .frame(width: 1000)
            VStack(alignment: .leading, spacing: 18) {
                if let picked {
                    Text(picked.longName).font(Palette.heading(36)).foregroundStyle(Palette.text)
                    if picked.entries.isEmpty {
                        TVNote(text: "Nothing from your Watchlist comes out this day.")
                    }
                    ScrollView {
                        VStack(spacing: 14) {
                            ForEach(Array(picked.entries.enumerated()), id: \.offset) { _, entry in
                                TVRow {
                                    model.open(Opening(id: entry.id, kind: entry.kind, title: entry.title,
                                                       video: entry.video.isEmpty ? nil : entry.video))
                                } content: {
                                    RoundedRectangle(cornerRadius: 3).fill(Palette.accent).frame(width: 6, height: 44)
                                    VStack(alignment: .leading, spacing: 4) {
                                        Text(entry.title).font(Palette.font(26, .semibold)).foregroundStyle(Palette.text).lineLimit(1)
                                        Text(entry.line).font(Palette.font(22)).foregroundStyle(Palette.textDim).lineLimit(1)
                                    }
                                    Spacer(minLength: 0)
                                    if !entry.time.isEmpty {
                                        Text(entry.time).font(Palette.font(24, .semibold)).foregroundStyle(Palette.accent)
                                    }
                                    if entry.watched { WatchedTick(size: 34) }
                                }
                                .accessibilityLabel(entry.title + (entry.label.isEmpty ? "" : ", \(entry.label)")
                                    + (entry.watched ? ", watched" : ""))
                            }
                        }
                        .padding(8)
                    }
                    .scrollClipDisabled()
                    .focusSection()
                }
            }
            .frame(maxWidth: .infinity, alignment: .topLeading)
        }
        .padding(.horizontal, TV.side)
        .padding(.vertical, 40)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .onAppear { calendar.appeared() }
    }
}

/// The squares, seven across under the days' names.
private struct TVDayGrid: View {
    let page: CalendarRules.Page
    let picked: Day?
    let pick: (Day) -> Void

    var body: some View {
        let columns = Array(repeating: GridItem(.flexible(), spacing: 14), count: 7)
        VStack(spacing: 14) {
            LazyVGrid(columns: columns, spacing: 14) {
                ForEach(CalendarRules.weekdays, id: \.self) { name in
                    Text(name).font(Palette.font(20, .semibold)).foregroundStyle(Palette.textDim)
                }
            }
            .accessibilityHidden(true)
            LazyVGrid(columns: columns, spacing: 14) {
                ForEach(page.squares) { square in
                    TVDaySquare(square: square, picked: square.day == picked) { pick(square.day) }
                }
            }
        }
    }
}

/// One day. Moving onto it with the remote picks it, so its releases show
/// beside the grid as the remote moves.
private struct TVDaySquare: View {
    let square: CalendarRules.Square
    let picked: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) { Face(square: square, picked: picked, focusedOn: action) }
            .buttonStyle(TVPlainStyle())
            .accessibilityLabel(square.longName + (square.entries.isEmpty ? "" : ", \(square.entries.count) coming out"))
            .accessibilityAddTraits(picked ? .isSelected : [])
    }

    private struct Face: View {
        let square: CalendarRules.Square
        let picked: Bool
        let focusedOn: () -> Void
        @Environment(\.isFocused) private var focused

        var body: some View {
            VStack(spacing: 8) {
                Text("\(square.day.day)")
                    .font(Palette.font(26, square.today || picked ? .bold : .medium))
                    .foregroundStyle(focused ? Palette.onAccent
                        : square.today || picked ? Palette.accent : square.inMonth ? Palette.text : Palette.textDim)
                    .opacity(square.inMonth || focused ? 1 : 0.5)
                Group {
                    if square.allWatched {
                        WatchedTick(size: 24)
                    } else if !square.entries.isEmpty {
                        HStack(spacing: 5) {
                            ForEach(0 ..< min(square.entries.count, 3), id: \.self) { _ in
                                Circle().fill(focused ? Palette.onAccent : Palette.accent).frame(width: 10, height: 10)
                            }
                        }
                        .opacity(square.inMonth || focused ? 1 : 0.55)
                    } else {
                        Color.clear
                    }
                }
                .frame(height: 24)
            }
            .frame(maxWidth: .infinity, minHeight: 86)
            .background(RoundedRectangle(cornerRadius: 14).fill(
                focused ? Color.white
                    : picked ? Palette.accent.opacity(0.22)
                    : square.today ? Palette.selected : square.inMonth ? Palette.surface : Color.clear))
            .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(
                picked || square.today ? Palette.accent : square.inMonth ? Palette.border : Color.clear,
                lineWidth: picked || square.today ? 2 : 1))
            .scaleEffect(focused ? 1.08 : 1)
            .animation(.easeOut(duration: 0.15), value: focused)
            .onChange(of: focused) { _, now in if now && !picked { focusedOn() } }
        }
    }
}
