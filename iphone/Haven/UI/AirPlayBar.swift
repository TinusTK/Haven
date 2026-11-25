import SwiftUI

/// What plays over AirPlay, above the tab bar on every page: its title,
/// where it plays and how far it got, −10 s / play-pause / +30 s / stop.
struct AirPlayBar: View {
    let session: AirPlaySession
    let stop: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text(session.barTitle)
                    .font(Palette.font(14, .semibold))
                    .foregroundStyle(Palette.text)
                    .lineLimit(1)
                Text(session.barStatus)
                    .font(Palette.font(12))
                    .foregroundStyle(session.failure == nil ? Palette.textDim : Palette.alert)
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
            if !session.ended {
                button("back", name: "Back 10 seconds") { session.skip(-10) }
                button(session.paused ? "play" : "pause", name: session.paused ? "Play" : "Pause") { session.togglePause() }
                button("forward", name: "Forward 30 seconds") { session.skip(30) }
                button("close", name: "Stop playing on AirPlay", action: stop)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
        .background(Palette.surface)
        .overlay(alignment: .top) { Rectangle().fill(Palette.border).frame(height: 1) }
        .accessibilityElement(children: .contain)
    }

    private func button(_ icon: String, name: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HavenIcon(name: icon, size: 18, tint: Palette.text)
                .frame(width: 36, height: 36)
                .contentShape(Rectangle())
        }
        .buttonStyle(PressedStyle())
        .accessibilityLabel(name)
    }
}
