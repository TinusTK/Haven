import SwiftUI

/// The player page: mpv's picture, and over it the controls. A tap shows
/// or hides them; they hide by themselves after 3 s unless something keeps
/// them up (paused, loading, a list open, an error, the next episode).
struct PlayerView: View {
    let player: PlayerModel
    @State private var shownAt = Date()

    private var pinned: Bool {
        player.paused || player.busy || player.menu != .none || !player.error.isEmpty || player.nextCountdown != nil
    }

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()
            MPVHost()
                .ignoresSafeArea()
                .accessibilityHidden(true)
            // The picture: a tap shows the controls, or hides them.
            Color.black.opacity(0.001)
                .ignoresSafeArea()
                .onTapGesture {
                    if player.menu != .none {
                        player.menu = .none
                    } else {
                        player.controlsShown.toggle()
                        shownAt = Date()
                    }
                }
            if player.busy && player.error.isEmpty {
                Text("Loading…")
                    .font(Palette.font(16))
                    .foregroundStyle(.white)
                    .offset(y: 80)
            }
            if !player.error.isEmpty {
                Text(player.error)
                    .font(Palette.font(15))
                    .foregroundStyle(.white)
                    .multilineTextAlignment(.center)
                    .padding(24)
                    .frame(maxWidth: 520)
                    .background(RoundedRectangle(cornerRadius: 12).fill(Color(hex: 0x151821).opacity(0.94)))
                    .padding(.horizontal, 32)
            }
            if player.controlsShown || pinned {
                Controls(player: player)
                    .transition(.opacity)
            }
            if player.menu != .none {
                MenuPanel(player: player)
            }
            if let countdown = player.nextCountdown {
                NextCard(player: player, countdown: countdown)
            }
        }
        .animation(.easeOut(duration: 0.2), value: player.controlsShown || pinned)
        .statusBarHidden(true)
        .persistentSystemOverlays(.hidden)
        .preferredColorScheme(.dark)
        .task(id: "\(player.controlsShown)|\(pinned)|\(shownAt)") {
            guard player.controlsShown, !pinned else { return }
            try? await Task.sleep(for: .seconds(PlayerRules.hideAfter))
            if !Task.isCancelled { player.controlsShown = false }
        }
    }
}

/// mpv's own view, one for the whole run.
private struct MPVHost: UIViewRepresentable {
    func makeUIView(context: Context) -> MPVView { MPV.shared.view }
    func updateUIView(_ view: MPVView, context: Context) {}
}

private struct Controls: View {
    let player: PlayerModel

    var body: some View {
        VStack(spacing: 0) {
            // Top: back, what is playing, how the stream comes in, when it ends.
            HStack(spacing: 14) {
                PlayerButton(label: "‹ Back", name: "Back") { player.close() }
                VStack(alignment: .leading, spacing: 2) {
                    Text(player.title).font(Palette.font(17, .semibold)).foregroundStyle(.white).lineLimit(1)
                    if !player.subtitle.isEmpty {
                        Text(player.subtitle).font(Palette.font(13)).foregroundStyle(.white.opacity(0.7)).lineLimit(1)
                    }
                }
                Spacer(minLength: 0)
                VStack(alignment: .trailing, spacing: 2) {
                    if !player.net.isEmpty {
                        Text(player.net)
                            .font(Palette.font(13, .semibold))
                            .foregroundStyle(.white)
                            .accessibilityLabel("The stream: \(player.net)")
                    }
                    if !player.endsAt.isEmpty {
                        Text(player.endsAt).font(Palette.font(12)).foregroundStyle(.white.opacity(0.7))
                    }
                }
            }
            .padding(16)
            .background(LinearGradient(colors: [.black.opacity(0.75), .clear], startPoint: .top, endPoint: .bottom).ignoresSafeArea())
            Spacer(minLength: 0)
            // Middle: back 10 seconds, play or pause, forward 30 seconds.
            if player.error.isEmpty {
                HStack(spacing: 40) {
                    if player.duration > 0 {
                        RoundButton(icon: "back", caption: "10", size: 64, name: "Back 10 seconds") { player.skip(-10) }
                    }
                    RoundButton(icon: player.paused ? "play" : "pause", caption: nil, size: 84,
                                name: player.paused ? "Play" : "Pause") { player.togglePause() }
                    if player.duration > 0 {
                        RoundButton(icon: "forward", caption: "30", size: 64, name: "Forward 30 seconds") { player.skip(30) }
                    }
                }
            }
            Spacer(minLength: 0)
            // Bottom: the position and the buttons.
            if player.error.isEmpty {
                VStack(spacing: 8) {
                    HStack(spacing: 12) {
                        Text(player.elapsedText)
                            .font(Palette.font(13, .semibold).monospacedDigit())
                            .foregroundStyle(.white)
                            .frame(minWidth: 52, alignment: .leading)
                        SeekBar(duration: player.duration, position: player.position, buffered: player.buffered) {
                            player.seek(to: $0)
                        }
                        Text(player.remainingText)
                            .font(Palette.font(13).monospacedDigit())
                            .foregroundStyle(.white.opacity(0.7))
                            .frame(minWidth: 52, alignment: .trailing)
                    }
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 6) {
                            let silent = player.preferences.muted || player.preferences.volume <= 0
                            PlayerButton(label: silent ? "Muted" : "Volume", name: player.preferences.muted ? "Turn the sound on" : "Mute",
                                         active: silent) { player.toggleMute() }
                            Slider(value: Binding(get: { player.preferences.muted ? 0 : player.preferences.volume },
                                                  set: { player.setVolume($0) }), in: 0 ... 1)
                                .tint(Palette.accent)
                                .frame(width: 110)
                                .accessibilityLabel("Volume")
                            if player.audioTracks.count > 1 {
                                PlayerButton(label: "Audio", active: player.menu == .audio) { player.toggleMenu(.audio) }
                            }
                            PlayerButton(label: "Subtitles", active: player.menu == .subtitles) { player.toggleMenu(.subtitles) }
                            PlayerButton(label: player.speed == 1 ? "Speed" : "\(PlayerRules.formatted(player.speed))×", name: "Speed",
                                         active: player.menu == .speed || player.speed != 1) { player.toggleMenu(.speed) }
                            PlayerButton(label: "Picture", active: player.menu == .picture || player.fit != .whole) {
                                player.toggleMenu(.picture)
                            }
                            PlayerButton(label: "Info", active: player.menu == .info) { player.toggleMenu(.info) }
                            if !player.nextTitle.isEmpty {
                                PlayerButton(label: "Next ›", name: "Next episode: \(player.nextTitle)") { player.goNext() }
                            }
                            AirPlayButton(tint: .white, active: Palette.accent)
                                .frame(width: 38, height: 38)
                                .accessibilityLabel("AirPlay")
                        }
                    }
                }
                .padding(.horizontal, 20)
                .padding(.bottom, 12)
                .padding(.top, 24)
                .background(LinearGradient(colors: [.clear, .black.opacity(0.85)], startPoint: .top, endPoint: .bottom).ignoresSafeArea())
            }
        }
    }
}

/// A word button on the player's bars.
private struct PlayerButton: View {
    let label: String
    var name: String?
    var active = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(label)
                .font(Palette.font(14, .semibold))
                .foregroundStyle(active ? Palette.accent : .white)
                .lineLimit(1)
                .padding(.horizontal, 12)
                .frame(height: 38)
                .background(Capsule().fill(Color.white.opacity(active ? 0.16 : 0.1)))
        }
        .buttonStyle(PressedStyle())
        .accessibilityLabel(name ?? label)
    }
}

/// A round button in the middle of the picture.
private struct RoundButton: View {
    let icon: String
    let caption: String?
    let size: CGFloat
    let name: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: 1) {
                HavenIcon(name: icon, size: size * 0.38, tint: .white, stroke: 2.4)
                if let caption {
                    Text(caption).font(Palette.font(11, .bold)).foregroundStyle(.white)
                }
            }
            .frame(width: size, height: size)
            .background(Circle().fill(Color.black.opacity(0.45)))
            .overlay(Circle().strokeBorder(Color.white.opacity(0.25), lineWidth: 1))
        }
        .buttonStyle(PressedStyle())
        .accessibilityLabel(name)
    }
}

/// The position: what has arrived in a lighter shade, a gold part for what
/// was played, and a bubble with the time while dragging.
private struct SeekBar: View {
    let duration: Double
    let position: Double
    let buffered: Double
    let seek: (Double) -> Void
    @State private var dragging: Double?

    var body: some View {
        GeometryReader { geometry in
            let width = max(geometry.size.width, 1)
            let shown = dragging ?? position
            let share = duration > 0 ? min(max(shown / duration, 0), 1) : 0
            let arrived = duration > 0 ? min(max(buffered / duration, 0), 1) : 0
            ZStack(alignment: .leading) {
                Capsule().fill(Color.white.opacity(0.22)).frame(height: 4)
                Capsule().fill(Color.white.opacity(0.4)).frame(width: width * arrived, height: 4)
                Capsule().fill(Palette.accent).frame(width: width * share, height: 4)
                Circle().fill(Palette.accent).frame(width: dragging == nil ? 14 : 20).offset(x: width * share - (dragging == nil ? 7 : 10))
                if let dragging {
                    Text(clockText(dragging))
                        .font(Palette.font(12, .bold).monospacedDigit())
                        .foregroundStyle(.black)
                        .padding(.horizontal, 8)
                        .frame(height: 24)
                        .background(Capsule().fill(.white))
                        .offset(x: min(max(width * share - 28, 0), width - 56), y: -28)
                }
            }
            .frame(height: 30)
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 0)
                .onChanged { drag in
                    guard duration > 0 else { return }
                    dragging = min(max(drag.location.x / width, 0), 1) * duration
                }
                .onEnded { _ in
                    if let dragging { seek(dragging) }
                    dragging = nil
                })
        }
        .frame(height: 30)
        .accessibilityElement()
        .accessibilityLabel("Position")
        .accessibilityValue("\(clockText(position)) of \(clockText(duration))")
        .accessibilityAdjustableAction { direction in
            switch direction {
            case .increment: seek(min(position + 30, duration))
            case .decrement: seek(max(position - 10, 0))
            @unknown default: break
            }
        }
    }
}

/// The open list: audio, subtitles, speed, picture or info.
private struct MenuPanel: View {
    let player: PlayerModel

    var body: some View {
        VStack {
            Spacer()
            VStack(alignment: .leading, spacing: 4) {
                Text(heading)
                    .font(Palette.font(12, .semibold))
                    .foregroundStyle(.white.opacity(0.7))
                    .padding(.horizontal, 8)
                    .padding(.top, 4)
                ScrollView {
                    VStack(alignment: .leading, spacing: 2) { rows }
                }
            }
            .padding(8)
            .frame(maxWidth: 360, maxHeight: 380)
            .background(RoundedRectangle(cornerRadius: 12).fill(Color(hex: 0x151821).opacity(0.96)))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.white.opacity(0.15), lineWidth: 1))
            .padding(.horizontal, 16)
            .padding(.bottom, 130)
        }
        .frame(maxWidth: .infinity, alignment: .trailing)
    }

    private var heading: String {
        switch player.menu {
        case .audio: "Audio"
        case .subtitles: "Subtitles"
        case .speed: "Speed"
        case .picture: "Picture"
        case .info, .none: "About what is playing"
        }
    }

    @ViewBuilder private var rows: some View {
        switch player.menu {
        case .audio:
            ForEach(player.audioTracks) { track in
                Choice(label: track.label, selected: track.selected) { player.chooseAudio(track.id) }
            }
        case .subtitles:
            Choice(label: "Off", selected: !player.anySubtitle) { player.chooseSubtitle(nil) }
            ForEach(player.subtitleTracks) { track in
                Choice(label: track.label, selected: track.selected) { player.chooseSubtitle(track.id) }
            }
            if player.subtitleTracks.isEmpty {
                Text("This has no subtitles of its own.").font(Palette.font(13)).foregroundStyle(.white.opacity(0.7)).padding(10)
            }
            Step(label: "Size", value: "\(Int((player.preferences.subtitleScale * 100).rounded())) %") { player.stepSubtitleScale($0) }
            Step(label: "Shown later by", value: "\(PlayerRules.formatted((player.subtitleDelay * 10).rounded() / 10)) s") {
                player.stepSubtitleDelay($0)
            }
            if !player.subtitleTracks.isEmpty {
                Text("Out of step? As someone starts to speak:")
                    .font(Palette.font(12))
                    .foregroundStyle(.white.opacity(0.7))
                    .padding(.horizontal, 10)
                    .padding(.top, 6)
                HStack(spacing: 6) {
                    PlayerButton(label: "Show next line now") { player.syncSubtitles(1) }
                    PlayerButton(label: "Line before") { player.syncSubtitles(-1) }
                    PlayerButton(label: "Reset") { player.syncSubtitles(0) }
                }
                .padding(.horizontal, 6)
            }
        case .speed:
            ForEach(PlayerRules.speeds, id: \.self) { speed in
                Choice(label: PlayerRules.speedLabel(speed), selected: player.speed == speed) { player.setSpeed(speed) }
            }
        case .picture:
            ForEach(MPV.Fit.allCases, id: \.rawValue) { fit in
                Choice(label: PlayerRules.fits[fit.rawValue], selected: player.fit == fit) { player.setFit(fit) }
            }
        case .info, .none:
            Text(player.info.isEmpty ? "Nothing is known yet." : player.info)
                .font(Palette.font(13))
                .foregroundStyle(.white)
                .padding(10)
        }
    }
}

private struct Choice: View {
    let label: String
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack {
                Text(label).font(Palette.font(14)).foregroundStyle(.white).multilineTextAlignment(.leading)
                Spacer(minLength: 8)
                if selected { HavenIcon(name: "check", size: 16, tint: Palette.accent) }
            }
            .padding(.horizontal, 10)
            .frame(minHeight: 40)
            .background(RoundedRectangle(cornerRadius: 8).fill(selected ? Color.white.opacity(0.08) : .clear))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

private struct Step: View {
    let label: String
    let value: String
    let step: (Int) -> Void

    var body: some View {
        HStack(spacing: 8) {
            Text(label).font(Palette.font(14)).foregroundStyle(.white)
            Spacer()
            PlayerButton(label: "−", name: "\(label): less") { step(-1) }
            Text(value).font(Palette.font(14, .semibold).monospacedDigit()).foregroundStyle(.white).frame(minWidth: 56)
            PlayerButton(label: "+", name: "\(label): more") { step(1) }
        }
        .padding(.horizontal, 10)
        .frame(minHeight: 44)
    }
}

/// The next episode is about to start by itself.
private struct NextCard: View {
    let player: PlayerModel
    let countdown: Int

    var body: some View {
        VStack {
            Spacer()
            VStack(alignment: .leading, spacing: 10) {
                Text("Next episode in \(countdown) s")
                    .font(Palette.font(13, .semibold))
                    .foregroundStyle(Palette.accent)
                Text(player.nextTitle).font(Palette.font(16, .semibold)).foregroundStyle(.white).lineLimit(2)
                HStack(spacing: 8) {
                    PlayerButton(label: "▶  Play now") { player.goNext() }
                    PlayerButton(label: "Cancel") { player.cancelNext() }
                }
            }
            .padding(16)
            .frame(maxWidth: 380, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 12).fill(Color(hex: 0x151821).opacity(0.96)))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.white.opacity(0.15), lineWidth: 1))
            .padding(.horizontal, 16)
            .padding(.bottom, 130)
        }
        .frame(maxWidth: .infinity, alignment: .trailing)
    }
}
