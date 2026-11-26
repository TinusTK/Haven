import SwiftUI

/// The pages Apple TV has, in the iPhone's order.
extension Page {
    static let onTV: [Page] = [.home, .explore, .library, .liveTV, .calendar, .settings]
}

/// Haven's screen on Apple TV: the pages under tvOS's own tab bar along the
/// top, and over them a title's page and the player. Menu on the remote
/// steps back: player, then title page, then out of Haven.
struct TVRootView: View {
    let core: Result<Core, Error>
    @State private var model: AppModel?

    var body: some View {
        Group {
            switch core {
            case let .success(core):
                if let model {
                    TVShell(model: model)
                        .preferredColorScheme(model.dark ? .dark : .light)
                } else {
                    Palette.background.ignoresSafeArea()
                        .onAppear {
                            let started = AppModel(core: core)
                            if !Page.onTV.contains(started.page) { started.select(.home) }
                            model = started
                            started.start()
                        }
                }
            case let .failure(error):
                VStack(spacing: 24) {
                    Image("Halo").resizable().scaledToFit().frame(width: 200, height: 200)
                    Text("Haven could not start").font(Palette.heading(44)).foregroundStyle(Palette.text)
                    TVNote(text: error.localizedDescription)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(Palette.background.ignoresSafeArea())
            }
        }
    }
}

private struct TVShell: View {
    let model: AppModel

    private var titleShown: Bool { model.opening != nil }

    var body: some View {
        ZStack {
            Palette.background.ignoresSafeArea()
            TabView(selection: Binding(get: { model.page }, set: { model.select($0) })) {
                ForEach(Page.onTV) { page in
                    TVPage(model: model, page: page)
                        .tabItem { Text(page.label) }
                        .tag(page)
                }
            }
            .disabled(titleShown || model.player.open || !model.splashGone || model.whatsNewShown)
            if titleShown {
                TVTitlePage(model: model, page: model.library.title)
                    .disabled(model.player.open)
                    .transition(.opacity)
            }
            if model.player.open {
                TVPlayerView(player: model.player)
                    .transition(.opacity)
            }
            if !model.splashGone {
                LoadingScreen(progress: model.splashProgress)
                    .scaleEffect(1.6)
                    .opacity(model.splashShown ? 1 : 0)
            }
            if model.whatsNewShown, let notes = model.notes {
                TVWhatsNew(notes: notes) { model.whatsNewDone(dontShowAgain: $0) }
                    .transition(.opacity)
            }
        }
        .animation(.easeOut(duration: 0.2), value: model.player.open)
        .animation(.easeOut(duration: 0.2), value: titleShown)
    }
}

/// One tab's page.
private struct TVPage: View {
    let model: AppModel
    let page: Page

    var body: some View {
        switch page {
        case .explore: TVExplorePage(model: model, explore: model.library.explore)
        case .library: TVLibraryPage(model: model)
        case .calendar: TVCalendarPage(model: model, calendar: model.calendar)
        case .liveTV: TVLiveTVPage(model: model, live: model.live)
        case .settings: TVSettingsPage(model: model, settings: model.settingsPage)
        default: TVHomePage(model: model)
        }
    }
}

/// What's New, on the television.
private struct TVWhatsNew: View {
    let notes: WhatsNew.Notes
    let done: (_ dontShowAgain: Bool) -> Void

    var body: some View {
        VStack(spacing: 30) {
            Text("What's New").font(Palette.heading(60)).foregroundStyle(Palette.text)
            Text("Haven \(notes.version)").font(Palette.font(26)).foregroundStyle(Palette.textDim)
            ScrollView {
                VStack(alignment: .leading, spacing: 24) {
                    ForEach(notes.sections) { section in
                        VStack(alignment: .leading, spacing: 12) {
                            Text(section.title).font(Palette.heading(32)).foregroundStyle(Palette.accent)
                            ForEach(Array(section.lines.enumerated()), id: \.offset) { _, line in
                                Text("•  \(line)").font(Palette.font(28)).foregroundStyle(Palette.text)
                            }
                        }
                    }
                }
                .frame(maxWidth: 1100, alignment: .leading)
            }
            HStack(spacing: 30) {
                TVChip(label: "Done", selected: true) { done(false) }
                TVChip(label: "Don't show again") { done(true) }
            }
        }
        .padding(80)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Palette.background.ignoresSafeArea())
    }
}
