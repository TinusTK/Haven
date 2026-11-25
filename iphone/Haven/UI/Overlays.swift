import SwiftUI

/// The loading screen, over everything while Haven starts: the halo turning
/// slowly with a pulse of gold light behind it, "Haven", and a bar with
/// "Loading… NN%" (then "Ready"). Nothing underneath can be touched.
struct LoadingScreen: View {
    let progress: Double

    var body: some View {
        TimelineView(.animation) { timeline in
            let seconds = timeline.date.timeIntervalSinceReferenceDate
            // A slow pulse: 0 to 1 and back every 1.6 s.
            let beat = (sin(seconds / 1.6 * 2 * .pi) + 1) / 2
            GeometryReader { geometry in
                let middle = geometry.size.height / 2
                ZStack {
                    Palette.background.ignoresSafeArea()
                    RadialGradient(colors: [Palette.accent.opacity(0.10 + 0.14 * beat), .clear],
                                   center: .center, startRadius: 0, endRadius: 105)
                        .frame(width: 300, height: 300)
                        .position(x: geometry.size.width / 2, y: middle - 60)
                    Image("Halo")
                        .resizable()
                        .scaledToFit()
                        .frame(width: 190, height: 190)
                        .rotationEffect(.degrees(seconds.truncatingRemainder(dividingBy: 12) / 12 * 360))
                        .opacity(0.85 + 0.15 * beat)
                        .position(x: geometry.size.width / 2, y: middle - 60)
                    Text("Haven")
                        .font(Palette.heading(30))
                        .foregroundStyle(Palette.text)
                        .position(x: geometry.size.width / 2, y: middle + 62 + 18)
                    bar(width: min(geometry.size.width - 80, 260))
                        .position(x: geometry.size.width / 2, y: middle + 118 + 3)
                    Text(progress >= 1 ? "Ready" : "Loading…  \(Int((progress * 100).rounded()))%")
                        .font(Palette.font(13))
                        .foregroundStyle(Palette.textDim)
                        .position(x: geometry.size.width / 2, y: middle + 136 + 9)
                }
            }
        }
        .contentShape(Rectangle())
        .onTapGesture {}
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Loading")
        .accessibilityValue("\(Int((progress * 100).rounded()))%")
    }

    private func bar(width: CGFloat) -> some View {
        ZStack(alignment: .leading) {
            Capsule().fill(Palette.border)
            Capsule().fill(Palette.accent)
                .frame(width: width * min(max(progress, 0), 1))
                .animation(.linear(duration: 0.16), value: progress)
        }
        .frame(width: width, height: 6)
    }
}

/// "What's New": this version's notes after the loading screen, with
/// "Don't show again" and Done.
struct WhatsNewScreen: View {
    let notes: WhatsNew.Notes
    let done: (_ dontShowAgain: Bool) -> Void
    @State private var hide = false

    var body: some View {
        VStack(spacing: 14) {
            VStack(spacing: 2) {
                Text("What's New")
                    .font(Palette.heading(40))
                    .foregroundStyle(Palette.text)
                Text("Haven \(notes.version)")
                    .font(Palette.font(14))
                    .foregroundStyle(Palette.textDim)
            }
            ScrollView {
                VStack(spacing: 12) {
                    ForEach(notes.sections) { section in
                        card(section)
                    }
                }
                .frame(maxWidth: 560)
                .frame(maxWidth: .infinity)
                .padding(.bottom, 4)
            }
            Button {
                hide.toggle()
            } label: {
                HStack(spacing: 10) {
                    RoundedRectangle(cornerRadius: 6)
                        .strokeBorder(hide ? Palette.accent : Palette.textDim, lineWidth: 2)
                        .background(RoundedRectangle(cornerRadius: 6).fill(hide ? Palette.accent : .clear))
                        .overlay { if hide { HavenIcon(name: "check", size: 18, tint: Palette.onAccent) } }
                        .frame(width: 24, height: 24)
                    Text("Don't show again")
                        .font(Palette.font(16, .semibold))
                        .foregroundStyle(Palette.text)
                }
                .padding(.horizontal, 12)
                .frame(height: 40)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Don't show again")
            .accessibilityAddTraits(hide ? [.isButton, .isSelected] : .isButton)
            PrimaryButton(text: "Done") { done(hide) }
                .frame(width: 220)
        }
        .padding(.top, 20)
        .padding(.bottom, 16)
        .padding(.horizontal, 16)
        .background(Palette.background.ignoresSafeArea())
    }

    private func card(_ section: WhatsNew.Notes.Section) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(section.title)
                .font(Palette.heading(18))
                .foregroundStyle(Palette.onAccent)
                .padding(.horizontal, 14)
                .frame(maxWidth: .infinity, minHeight: 40, alignment: .leading)
                .background(Palette.accent)
            VStack(alignment: .leading, spacing: 8) {
                ForEach(Array(section.lines.enumerated()), id: \.offset) { _, line in
                    HStack(alignment: .top, spacing: 10) {
                        RoundedRectangle(cornerRadius: 2)
                            .fill(Palette.accent)
                            .frame(width: 7, height: 7)
                            .padding(.top, 6)
                        Text(line)
                            .font(Palette.font(14))
                            .foregroundStyle(Palette.text)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(Palette.surface)
        .clipShape(RoundedRectangle(cornerRadius: 14))
        .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(Palette.border, lineWidth: 1))
    }
}
