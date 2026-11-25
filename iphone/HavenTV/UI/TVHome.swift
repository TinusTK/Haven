import SwiftUI

/// Home on the television: the banner, Upcoming, Continue Watching, the
/// Watchlist, then the shelves (your media servers first, then the
/// catalogues, then TMDB), each a row the remote moves along.
struct TVHomePage: View {
    let model: AppModel
    var library: Library { model.library }

    var body: some View {
        Group {
            if library.homeHasSources {
                filled
            } else {
                VStack(alignment: .leading, spacing: 30) {
                    Text("Home").font(Palette.heading(56)).foregroundStyle(Palette.text)
                    TVNote(text: HomeText.empty)
                    TVChip(label: "Open Settings", selected: true) { model.select(.settings) }
                }
                .frame(maxWidth: 1200, maxHeight: .infinity, alignment: .topLeading)
                .padding(TV.side)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .onAppear { model.homeAppeared(true) }
        .onDisappear { model.homeAppeared(false) }
    }

    private var filled: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 56) {
                if !library.hero.isEmpty {
                    TVHero(slides: library.hero, showing: model.page == .home && !model.detailOpen, pause: library.homePause,
                           firstPicture: { library.heroFirstPicture = true }) { slide in
                        model.open(Opening(id: slide.id, kind: slide.kind, title: slide.title))
                    }
                }
                if !library.upcoming.isEmpty {
                    TVRowBlock(title: "Upcoming") {
                        ForEach(library.upcoming) { card in
                            // Counts down by the hour on the day.
                            TimelineView(.periodic(from: .now, by: 30)) { context in
                                TVWideCard(title: card.title, line: card.episode, picture: card.picture, badge: card.badge,
                                           corner: card.when(at: context.date), pause: library.homePause) {
                                    model.open(Opening(id: card.id, kind: card.kind, title: card.title,
                                                       video: card.video.isEmpty ? nil : card.video))
                                }
                            }
                        }
                    }
                }
                if !library.continueCards.isEmpty {
                    TVRowBlock(title: "Continue Watching") {
                        ForEach(library.continueCards) { card in
                            TVWideCard(title: card.title, line: card.episode, picture: card.picture, badge: card.badge,
                                       corner: card.left > 0 ? "\(card.left) left" : "", progress: card.progress,
                                       pause: library.homePause) {
                                model.open(Opening(id: card.id, kind: card.kind, title: card.title, video: card.video))
                            }
                            .contextMenu {
                                Button("Remove from Continue Watching", role: .destructive) {
                                    library.removeContinue(kind: card.kind, id: card.id)
                                }
                            }
                        }
                    }
                }
                if !library.watchlist.isEmpty {
                    TVPosterRow(title: "Watchlist", detail: "", items: library.watchlist.map {
                        MetaPreview(id: $0.titleID, kind: $0.kind, name: $0.title, poster: $0.poster, releaseInfo: $0.year)
                    }, model: model, pause: library.homePause)
                }
                if library.homeBoard.shelves.isEmpty && library.serverBoard.shelves.isEmpty && library.catalogBoard.shelves.isEmpty {
                    TVNote(text: library.servers.enabled.isEmpty ? "Nothing to show here yet." : "Waiting for your media servers…")
                }
                ForEach(library.serverBoard.shelves) { shelf in
                    if !(shelf.items.isEmpty && !shelf.loading && shelf.error.isEmpty) {
                        TVShelfRow(shelf: shelf, model: model)
                    }
                }
                ForEach(library.homeBoard.shelves) { TVShelfRow(shelf: $0, model: model) }
                ForEach(library.catalogBoard.shelves) { TVShelfRow(shelf: $0, model: model) }
            }
            .padding(.horizontal, TV.side)
            .padding(.vertical, 40)
        }
    }
}

enum HomeText {
    static let empty = "Home fills with shelves of films and series from your media servers and the TMDB catalogue. Add a media server or a TMDB key in Settings."
}

/// The banner: a few titles in turn, large, every 9 s while Home shows.
private struct TVHero: View {
    let slides: [HomeTop.Slide]
    let showing: Bool
    let pause: Pause
    let firstPicture: () -> Void
    let open: (HomeTop.Slide) -> Void
    @State private var index = 0
    @FocusState private var focused: Bool

    var body: some View {
        let slide = slides[min(index, slides.count - 1)]
        Button { open(slide) } label: {
            ZStack(alignment: .bottomLeading) {
                RoundedRectangle(cornerRadius: 28).fill(Palette.surface)
                RemotePicture(url: slide.image, size: .background, pause: pause,
                              loaded: { if index == 0 { firstPicture() } }) { Color.clear }
                    .frame(height: 620)
                    .frame(maxWidth: .infinity)
                    .clipped()
                LinearGradient(stops: [.init(color: Color(hex: 0x0d0f14).opacity(0.92), location: 0),
                                       .init(color: .clear, location: 0.65)],
                               startPoint: .leading, endPoint: .trailing)
                VStack(alignment: .leading, spacing: 16) {
                    if let logo = slide.logo {
                        RemotePicture(url: logo, size: .logo, pause: pause) { title(slide) }
                            .scaledToFit()
                            .frame(maxWidth: 560, maxHeight: 160, alignment: .leading)
                    } else {
                        title(slide)
                    }
                    if !slide.facts.isEmpty {
                        Text(slide.facts).font(Palette.font(26, .semibold)).foregroundStyle(Color(white: 0.95))
                    }
                    if !slide.description.isEmpty {
                        Text(slide.description)
                            .font(Palette.font(24))
                            .foregroundStyle(Color(white: 0.85))
                            .lineLimit(3)
                            .frame(maxWidth: 820, alignment: .leading)
                    }
                    HStack(spacing: 10) {
                        ForEach(slides.indices, id: \.self) { at in
                            Capsule().fill(at == index ? Color.white : Color.white.opacity(0.35))
                                .frame(width: at == index ? 40 : 12, height: 12)
                        }
                    }
                    .padding(.top, 8)
                }
                .padding(50)
            }
            .frame(height: 620)
            .clipShape(RoundedRectangle(cornerRadius: 28))
        }
        .buttonStyle(TVFocusStyle(shape: AnyShape(RoundedRectangle(cornerRadius: 28)), lift: 1.02))
        .focused($focused)
        .animation(.easeInOut(duration: 0.4), value: index)
        // Left and right on the banner turn it by hand.
        .onMoveCommand { direction in
            guard focused, slides.count > 1 else { return }
            if direction == .right { index = (index + 1) % slides.count }
            if direction == .left { index = (index + slides.count - 1) % slides.count }
        }
        .task(id: "\(showing)|\(slides.count)|\(index)") {
            guard showing, slides.count > 1 else { return }
            try? await Task.sleep(for: .seconds(HomeTop.turnEvery))
            if !Task.isCancelled { index = (index + 1) % slides.count }
        }
    }

    private func title(_ slide: HomeTop.Slide) -> some View {
        Text(slide.title).font(Palette.font(60, .heavy)).foregroundStyle(.white).lineLimit(2)
    }
}

/// A heading and a row the remote moves along.
struct TVRowBlock<Content: View>: View {
    let title: String
    var detail = ""
    @ViewBuilder var content: () -> Content

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            TVHeading(text: title, detail: detail)
            ScrollView(.horizontal, showsIndicators: false) {
                LazyHStack(alignment: .top, spacing: 40) { content() }
                    .padding(.vertical, 24)
                    .padding(.horizontal, 6)
            }
            .scrollClipDisabled()
            .focusSection()
        }
    }
}

/// A shelf: its titles in a row, or why it has none.
struct TVShelfRow: View {
    let shelf: Shelf
    let model: AppModel
    var counted = true

    var body: some View {
        if shelf.items.isEmpty {
            VStack(alignment: .leading, spacing: 14) {
                TVHeading(text: shelf.title, detail: shelf.source)
                TVNote(text: shelf.loading ? "Loading…" : shelf.error.isEmpty ? "Nothing here." : shelf.error)
            }
        } else {
            TVPosterRow(title: shelf.title, detail: shelf.source, items: shelf.items, model: model,
                        pause: counted ? model.library.homePause : nil)
        }
    }
}

struct TVPosterRow: View {
    let title: String
    let detail: String
    let items: [MetaPreview]
    let model: AppModel
    var pause: Pause?

    var body: some View {
        TVRowBlock(title: title, detail: detail) {
            ForEach(items, id: \.id) { meta in
                TVPoster(meta: meta, model: model, pause: pause)
            }
        }
    }
}

/// A poster button: opens the title's page.
struct TVPoster: View {
    let meta: MetaPreview
    let model: AppModel
    var pause: Pause?
    var width: CGFloat = TV.poster

    var body: some View {
        Button {
            model.open(Opening(id: meta.id, kind: meta.kind, title: meta.name, poster: meta.poster))
        } label: {
            PosterCard(id: meta.id, title: meta.name, poster: meta.poster, rating: meta.rating,
                       watched: model.library.filmWatched(kind: meta.kind, id: meta.id), width: width, pause: pause)
                .scaleEffect(1)
        }
        .buttonStyle(TVPosterStyle())
    }
}

/// Posters lift and get a gold edge on the picture only (not the name).
struct TVPosterStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        Face(configuration: configuration)
    }

    private struct Face: View {
        let configuration: Configuration
        @Environment(\.isFocused) private var focused

        var body: some View {
            configuration.label
                .scaleEffect(focused ? 1.1 : 1, anchor: .bottom)
                .shadow(color: .black.opacity(focused ? 0.5 : 0), radius: 20, y: 12)
                .brightness(focused ? 0.04 : 0)
                .animation(.easeOut(duration: 0.15), value: focused)
        }
    }
}

/// A wide card (Upcoming, Continue Watching).
struct TVWideCard: View {
    let title: String
    let line: String
    let picture: String?
    let badge: String
    var corner = ""
    var progress: Double?
    var pause: Pause?
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: 0) {
                ZStack(alignment: .bottomLeading) {
                    RoundedRectangle(cornerRadius: 18).fill(Palette.surface)
                    RemotePicture(url: picture, size: .wide, pause: pause) { Color.clear }
                        .frame(width: TV.wide, height: TV.wide * 0.5625)
                        .clipped()
                    LinearGradient(colors: [.clear, .black.opacity(0.65)], startPoint: UnitPoint(x: 0.5, y: 0.45), endPoint: .bottom)
                    if !badge.isEmpty {
                        Text(badge).font(Palette.font(22, .heavy)).foregroundStyle(.white)
                            .padding(.leading, 20).padding(.bottom, progress == nil ? 18 : 26)
                    }
                    if !corner.isEmpty {
                        Text(corner)
                            .font(Palette.font(20, .bold))
                            .foregroundStyle(.white)
                            .padding(.horizontal, 12)
                            .frame(height: 36)
                            .background(Capsule().fill(Color.black.opacity(0.65)))
                            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)
                            .padding(14)
                    }
                    if let progress {
                        ProgressBar(share: progress).padding(.horizontal, 16).padding(.bottom, 12)
                    }
                }
                .frame(width: TV.wide, height: TV.wide * 0.5625)
                .clipShape(RoundedRectangle(cornerRadius: 18))
                Text(title).font(Palette.font(26, .semibold)).foregroundStyle(Palette.text).lineLimit(1).padding(.top, 14)
                Text(line).font(Palette.font(22)).foregroundStyle(Palette.textDim).lineLimit(1).padding(.top, 4)
            }
            .frame(width: TV.wide, alignment: .leading)
        }
        .buttonStyle(TVPosterStyle())
    }
}
