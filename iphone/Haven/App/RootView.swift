import SwiftUI

/// Haven's window: the page showing, the tab bar along the bottom, and over
/// them the loading screen and What's New. Content stays clear of the
/// status bar; the tab bar sits above the home bar.
struct RootView: View {
    let core: Result<Core, Error>
    @State private var model: AppModel?
    @State private var typing = Typing()

    var body: some View {
        Group {
            switch core {
            case let .success(core):
                if let model {
                    Shell(model: model)
                        .environment(typing)
                        .preferredColorScheme(model.dark ? .dark : .light)
                } else {
                    Palette.background.ignoresSafeArea()
                        .onAppear {
                            let started = AppModel(core: core)
                            model = started
                            started.start()
                        }
                }
            case let .failure(error):
                CouldNotStart(error: error)
            }
        }
    }
}

private struct Shell: View {
    let model: AppModel
    @Environment(Typing.self) private var typing

    var body: some View {
        ZStack {
            Palette.background.ignoresSafeArea()
            VStack(spacing: 0) {
                ZStack {
                    PageView(model: model)
                    if let opening = model.opening {
                        TitlePlaceholder(opening: opening) { model.closeTitle() }
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                TabBar(page: model.page, detailOpen: model.detailOpen) { model.select($0) }
                    .background(Palette.background.ignoresSafeArea(edges: .bottom))
            }
            .ignoresSafeArea(.keyboard)
            if let field = typing.field {
                OutsideShape(hole: field)
                    .fill(Color.white.opacity(0.001), style: FillStyle(eoFill: true))
                    .contentShape(OutsideShape(hole: field), eoFill: true)
                    .ignoresSafeArea()
                    .onTapGesture { typing.close() }
            }
            if !model.splashGone {
                LoadingScreen(progress: model.splashProgress)
                    .opacity(model.splashShown ? 1 : 0)
            }
            if model.whatsNewShown, let notes = model.notes {
                WhatsNewScreen(notes: notes) { model.whatsNewDone(dontShowAgain: $0) }
                    .transition(.opacity)
            }
        }
        .animation(.easeOut(duration: 0.2), value: model.whatsNewShown)
    }
}

/// Haven could not open its data (a database from a newer Haven, say).
private struct CouldNotStart: View {
    let error: Error

    var body: some View {
        VStack(spacing: 14) {
            Image("Halo").resizable().scaledToFit().frame(width: 120, height: 120)
            Text("Haven could not start")
                .font(Palette.heading(24))
                .foregroundStyle(Palette.text)
            Text(error.localizedDescription)
                .font(Palette.font(14))
                .foregroundStyle(Palette.textDim)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 24)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Palette.background.ignoresSafeArea())
    }
}
