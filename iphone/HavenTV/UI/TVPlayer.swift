import SwiftUI

/// The player on the television: mpv's picture, and the controls over it.
///
/// With the controls hidden: Play/Pause pauses, left and right skip back
/// 10 s and on 30 s, Select or up/down shows the controls, Menu leaves.
/// With them shown: the remote moves between the position bar (left and
/// right skip there too) and the buttons; Menu hides them. They hide by
/// themselves after a few seconds unless paused or a list is open.
struct TVPlayerView: View {
    let player: PlayerModel
    @State private var touched = Date()
    @FocusState private var focus: Spot?

    enum Spot: Hashable { case catcher, bar, button(Int), choice(Int) }

    private var pinned: Bool {
        player.paused || player.busy || player.menu != .none || !player.error.isEmpty || player.nextCountdown != nil
    }

    private var shown: Bool { player.controlsShown || pinned }

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()
            TVMPVHost().ignoresSafeArea().accessibilityHidden(true)
            if !shown {
                // Takes the remote while the controls are hidden.
                Button { show() } label: { Color.clear.frame(maxWidth: .infinity, maxHeight: .infinity) }
                    .buttonStyle(TVPlainStyle())
                    .focused($focus, equals: .catcher)
                    .onMoveCommand { direction in
                        switch direction {
                        case .left: player.skip(-10)
                        case .right: player.skip(30)
                        default: show()
                        }
                    }
                    .accessibilityLabel("Show the controls")
            }
            if player.busy && player.error.isEmpty {
                Text("Loading…").font(Palette.font(32)).foregroundStyle(.white)
            }
            if !player.error.isEmpty {
                Text(player.error)
                    .font(Palette.font(30))
                    .foregroundStyle(.white)
                    .multilineTextAlignment(.center)
                    .padding(40)
                    .frame(maxWidth: 1000)
                    .background(RoundedRectangle(cornerRadius: 20).fill(Color(hex: 0x151821).opacity(0.94)))
            }
            if shown {
                controls.transition(.opacity)
            }
            if player.menu != .none {
                menuPanel
            }
            if let countdown = player.nextCountdown {
                nextCard(countdown)
            }
        }
        .preferredColorScheme(.dark)
        .animation(.easeOut(duration: 0.25), value: shown)
        .onPlayPauseCommand {
            player.togglePause()
            touched = Date()
        }
        .onExitCommand {
            if player.menu != .none {
                player.menu = .none
            } else if shown && player.error.isEmpty && !pinned {
                player.controlsShown = false
            } else {
                player.close()
            }
        }
        .onAppear { focus = shown ? .button(0) : .catcher }
        .onChange(of: shown) { _, now in focus = now ? .bar : .catcher }
        .onChange(of: player.menu) { _, menu in focus = menu == .none ? .button(menuIndex) : .choice(0) }
        .onChange(of: focus) { _, _ in touched = Date() }
        .task(id: "\(player.controlsShown)|\(pinned)|\(touched)") {
            guard player.controlsShown, !pinned else { return }
            try? await Task.sleep(for: .seconds(PlayerRules.hideAfter + 2))
            if !Task.isCancelled { player.controlsShown = false }
        }
    }

    private func show() {
        player.controlsShown = true
        touched = Date()
    }

    /// The button that opened the list showing, to go back to.
    @State private var menuIndex = 0

    // MARK: Controls

    private var controls: some View {
        VStack(spacing: 0) {
            HStack(alignment: .top, spacing: 30) {
                VStack(alignment: .leading, spacing: 6) {
                    Text(player.title).font(Palette.font(40, .semibold)).foregroundStyle(.white).lineLimit(1)
                    if !player.subtitle.isEmpty {
                        Text(player.subtitle).font(Palette.font(28)).foregroundStyle(.white.opacity(0.75)).lineLimit(1)
                    }
                }
                Spacer(minLength: 0)
                VStack(alignment: .trailing, spacing: 6) {
                    if !player.net.isEmpty { Text(player.net).font(Palette.font(24, .semibold)).foregroundStyle(.white) }
                    if !player.endsAt.isEmpty { Text(player.endsAt).font(Palette.font(24)).foregroundStyle(.white.opacity(0.75)) }
                }
            }
            .padding(.horizontal, TV.side)
            .padding(.top, 50)
            .padding(.bottom, 80)
            .background(LinearGradient(colors: [.black.opacity(0.8), .clear], startPoint: .top, endPoint: .bottom))
            Spacer(minLength: 0)
            if player.error.isEmpty {
                VStack(spacing: 30) {
                    if player.duration > 0 {
                        HStack(spacing: 24) {
                            Text(player.elapsedText).font(Palette.font(26, .semibold).monospacedDigit()).foregroundStyle(.white)
                            TVSeekBar(player: player) { touched = Date() }
                                .focused($focus, equals: .bar)
                            Text(player.remainingText).font(Palette.font(26).monospacedDigit()).foregroundStyle(.white.opacity(0.75))
                        }
                    }
                    HStack(spacing: 20) {
                        button(0, player.paused ? "Play" : "Pause", icon: player.paused ? "play" : "pause") { player.togglePause() }
                        if player.duration > 0 {
                            button(1, "−10 s") { player.skip(-10) }
                            button(2, "+30 s") { player.skip(30) }
                        }
                        if player.audioTracks.count > 1 {
                            button(3, "Audio", active: player.menu == .audio) { open(.audio, 3) }
                        }
                        button(4, "Subtitles", active: player.menu == .subtitles) { open(.subtitles, 4) }
                        button(5, player.speed == 1 ? "Speed" : "\(PlayerRules.formatted(player.speed))×",
                               active: player.menu == .speed || player.speed != 1) { open(.speed, 5) }
                        button(6, "Picture", active: player.menu == .picture || player.fit != .whole) { open(.picture, 6) }
                        button(9, boostLabel, active: player.menu == .sound || player.preferences.volume > 1.001) { open(.sound, 9) }
                        button(7, "Info", active: player.menu == .info) { open(.info, 7) }
                        if !player.nextTitle.isEmpty {
                            button(8, "Next episode") { player.goNext() }
                        }
                        Spacer(minLength: 0)
                    }
                    .focusSection()
                }
                .padding(.horizontal, TV.side)
                .padding(.top, 80)
                .padding(.bottom, 60)
                .background(LinearGradient(colors: [.clear, .black.opacity(0.85)], startPoint: .top, endPoint: .bottom))
            } else {
                HStack {
                    button(0, "Back") { player.close() }
                    Spacer()
                }
                .padding(TV.side)
            }
        }
        .ignoresSafeArea()
    }

    /// The Sound button: how much louder, when boosted.
    private var boostLabel: String {
        player.preferences.volume > 1.001 ? "Sound \(Int((player.preferences.volume * 100).rounded()))%" : "Sound"
    }

    private func open(_ menu: PlayerModel.Menu, _ index: Int) {
        menuIndex = index
        player.toggleMenu(menu)
    }

    private func button(_ index: Int, _ label: String, icon: String? = nil, active: Bool = false,
                        action: @escaping () -> Void) -> some View {
        TVChip(label: label, selected: active, icon: icon) {
            action()
            touched = Date()
        }
        .focused($focus, equals: .button(index))
    }

    // MARK: Lists

    private var menuPanel: some View {
        HStack {
            Spacer()
            VStack(alignment: .leading, spacing: 10) {
                Text(heading).font(Palette.font(26, .semibold)).foregroundStyle(.white.opacity(0.75))
                ScrollView {
                    VStack(alignment: .leading, spacing: 10) { rows }
                        .padding(8)
                }
                .scrollClipDisabled()
            }
            .padding(30)
            .frame(width: 680)
            .frame(maxHeight: 760)
            .background(RoundedRectangle(cornerRadius: 24).fill(Color(hex: 0x151821).opacity(0.96)))
            .padding(.trailing, TV.side)
            .focusSection()
        }
    }

    private var heading: String {
        switch player.menu {
        case .audio: "Audio"
        case .subtitles: "Subtitles"
        case .speed: "Speed"
        case .picture: "Picture"
        case .sound: "Sound boost"
        case .info, .none: "About what is playing"
        }
    }

    @ViewBuilder private var rows: some View {
        switch player.menu {
        case .audio:
            ForEach(Array(player.audioTracks.enumerated()), id: \.element.id) { at, track in
                choice(at, track.label, track.selected) { player.chooseAudio(track.id) }
            }
        case .subtitles:
            choice(0, "Off", !player.anySubtitle) { player.chooseSubtitle(nil) }
            ForEach(Array(player.subtitleTracks.enumerated()), id: \.element.id) { at, track in
                choice(at + 1, track.label, track.selected) { player.chooseSubtitle(track.id) }
            }
            if player.subtitleTracks.isEmpty {
                TVNote(text: "This has no subtitles of its own.")
            }
            step("Size", "\(Int((player.preferences.subtitleScale * 100).rounded())) %") { player.stepSubtitleScale($0) }
            step("Shown later by", "\(PlayerRules.formatted((player.subtitleDelay * 10).rounded() / 10)) s") {
                player.stepSubtitleDelay($0)
            }
        case .speed:
            ForEach(Array(PlayerRules.speeds.enumerated()), id: \.offset) { at, speed in
                choice(at, PlayerRules.speedLabel(speed), player.speed == speed) { player.setSpeed(speed) }
            }
        case .picture:
            ForEach(MPV.Fit.allCases, id: \.rawValue) { fit in
                choice(fit.rawValue, PlayerRules.fits[fit.rawValue], player.fit == fit) { player.setFit(fit) }
            }
        case .sound:
            ForEach(Array(PlayerRules.Preferences.boosts.enumerated()), id: \.offset) { at, boost in
                choice(at, boost == 1 ? "Off (100%)" : "\(Int((boost * 100).rounded()))%",
                       abs(player.preferences.volume - boost) < 0.01) { player.setVolume(boost) }
            }
            TVNote(text: "Makes quiet films louder than the TV's own volume can. Very loud sound can crackle.")
        case .info, .none:
            Text(player.info.isEmpty ? "Nothing is known yet." : player.info)
                .font(Palette.font(24))
                .foregroundStyle(.white)
                .focusable()
                .focused($focus, equals: .choice(0))
        }
    }

    private func choice(_ index: Int, _ label: String, _ selected: Bool, action: @escaping () -> Void) -> some View {
        TVRow(selected: selected, action: action) {
            Text(label).font(Palette.font(26)).foregroundStyle(Palette.text).lineLimit(2)
            Spacer(minLength: 0)
            if selected { HavenIcon(name: "check", size: 28, tint: Palette.accent) }
        }
        .focused($focus, equals: .choice(index))
    }

    private func step(_ label: String, _ value: String, _ change: @escaping (Int) -> Void) -> some View {
        HStack(spacing: 16) {
            Text(label).font(Palette.font(24)).foregroundStyle(.white)
            Spacer()
            TVChip(label: "−") { change(-1) }
            Text(value).font(Palette.font(24, .semibold).monospacedDigit()).foregroundStyle(.white).frame(minWidth: 100)
            TVChip(label: "+") { change(1) }
        }
        .padding(.top, 8)
    }

    private func nextCard(_ countdown: Int) -> some View {
        VStack {
            Spacer()
            HStack {
                Spacer()
                VStack(alignment: .leading, spacing: 16) {
                    Text("Next episode in \(countdown) s").font(Palette.font(24, .semibold)).foregroundStyle(Palette.accent)
                    Text(player.nextTitle).font(Palette.font(30, .semibold)).foregroundStyle(.white).lineLimit(2)
                    HStack(spacing: 20) {
                        TVChip(label: "Play now", selected: true, icon: "play") { player.goNext() }
                        TVChip(label: "Cancel") { player.cancelNext() }
                    }
                }
                .padding(30)
                .frame(width: 640, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: 24).fill(Color(hex: 0x151821).opacity(0.96)))
                .focusSection()
            }
        }
        .padding(.trailing, TV.side)
        .padding(.bottom, 260)
    }
}

/// mpv's own view, one for the whole run.
private struct TVMPVHost: UIViewRepresentable {
    func makeUIView(context: Context) -> MPVView { MPV.shared.view }
    func updateUIView(_ view: MPVView, context: Context) {}
}

/// The position bar: what has arrived in a lighter shade and what was
/// played in gold. With the remote on it, left and right skip.
private struct TVSeekBar: View {
    let player: PlayerModel
    let touched: () -> Void
    @FocusState private var focused: Bool

    var body: some View {
        GeometryReader { geometry in
            let width = max(geometry.size.width, 1)
            let duration = max(player.duration, 1)
            let played = min(max(player.position / duration, 0), 1)
            let arrived = min(max(player.buffered / duration, 0), 1)
            ZStack(alignment: .leading) {
                Capsule().fill(Color.white.opacity(0.22))
                Capsule().fill(Color.white.opacity(0.4)).frame(width: width * arrived)
                Capsule().fill(Palette.accent).frame(width: width * played)
                Circle().fill(focused ? Color.white : Palette.accent)
                    .frame(width: focused ? 30 : 18, height: focused ? 30 : 18)
                    .offset(x: width * played - (focused ? 15 : 9))
            }
            .frame(height: focused ? 12 : 8)
            .frame(maxHeight: .infinity)
        }
        .frame(height: 40)
        .focusable()
        .focused($focused)
        .onMoveCommand { direction in
            if direction == .left { player.skip(-10) }
            if direction == .right { player.skip(30) }
            touched()
        }
        .animation(.easeOut(duration: 0.15), value: focused)
        .accessibilityLabel("Position")
        .accessibilityValue(player.elapsedText)
    }
}
