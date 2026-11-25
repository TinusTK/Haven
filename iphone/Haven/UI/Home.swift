import SwiftUI

/// Home: the banner, Upcoming, Continue Watching, the Watchlist, then the
/// shelves (your media servers first, then Cinemeta and add-ons, then
/// TMDB). Pull down to load it afresh.
struct HomePage: View {
    let model: AppModel
    var library: Library { model.library }

    var body: some View {
        Group {
            if library.homeHasSources {
                filled
            } else {
                empty
            }
        }
        .onAppear { model.homeAppeared(true) }
        .onDisappear { model.homeAppeared(false) }
    }

    private var filled: some View {
        GeometryReader { geometry in
            let width = geometry.size.width
            let wide = max((width - 32 - 14) / 2, 0)
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 28) {
                    if !library.hero.isEmpty {
                        HeroBanner(slides: library.hero, width: width - 32, showing: model.page == .home && !model.detailOpen,
                                   pause: library.homePause, firstPicture: { library.heroFirstPicture = true }) { slide in
                            model.open(Opening(id: slide.id, kind: slide.kind, title: slide.title))
                        }
                    }
                    if !library.upcoming.isEmpty {
                        VStack(alignment: .leading, spacing: 12) {
                            RowHeading(title: "Upcoming") { model.select(.calendar) }
                            ScrollView(.horizontal, showsIndicators: false) {
                                LazyHStack(spacing: 16) {
                                    ForEach(library.upcoming) { card in
                                        Button {
                                            model.open(Opening(id: card.id, kind: card.kind, title: card.title,
                                                               video: card.video.isEmpty ? nil : card.video))
                                        } label: {
                                            WideCard(title: card.title, line: card.episode, picture: card.picture, badge: card.badge,
                                                     width: wide, pause: library.homePause) {
                                                // Counts down by the hour on the day.
                                                TimelineView(.periodic(from: .now, by: 30)) { context in
                                                    WhenPill(text: card.when(at: context.date), soon: card.soon)
                                                }
                                            }
                                        }
                                        .buttonStyle(PressedStyle())
                                    }
                                }
                            }
                            .scrollClipDisabled()
                        }
                    }
                    if !library.continueCards.isEmpty {
                        VStack(alignment: .leading, spacing: 12) {
                            RowHeading(title: "Continue Watching") { model.openLibrary(3) }
                            ScrollView(.horizontal, showsIndicators: false) {
                                LazyHStack(spacing: 16) {
                                    ForEach(library.continueCards) { card in
                                        Button {
                                            model.open(Opening(id: card.id, kind: card.kind, title: card.title, video: card.video))
                                        } label: {
                                            WideCard(title: card.title, line: card.episode, picture: card.picture, badge: card.badge,
                                                     width: wide, progress: card.progress, pause: library.homePause) {
                                                if card.left > 0 { LeftPill(count: card.left) }
                                            }
                                        }
                                        .buttonStyle(PressedStyle())
                                        .contextMenu {
                                            Button("Remove from Continue Watching", role: .destructive) {
                                                library.removeContinue(kind: card.kind, id: card.id)
                                            }
                                        }
                                    }
                                }
                            }
                            .scrollClipDisabled()
                        }
                    }
                    if !library.watchlist.isEmpty {
                        VStack(alignment: .leading, spacing: 12) {
                            RowHeading(title: "Watchlist") { model.openLibrary(2) }
                            PosterRow(items: library.watchlist.map {
                                MetaPreview(id: $0.titleID, kind: $0.kind, name: $0.title, poster: $0.poster, releaseInfo: $0.year)
                            }, width: width - 32, model: model, pause: library.homePause)
                        }
                    }
                    if library.homeBoard.shelves.isEmpty && library.serverBoard.shelves.isEmpty && library.catalogBoard.shelves.isEmpty {
                        Text(emptyNote)
                            .font(Palette.font(14))
                            .foregroundStyle(Palette.textDim)
                    }
                    // Your own libraries first, then catalogues, then TMDB.
                    ForEach(library.serverBoard.shelves) { shelf in
                        if !(shelf.items.isEmpty && !shelf.loading && shelf.error.isEmpty) {
                            ShelfRow(shelf: shelf, width: width - 32, model: model)
                        }
                    }
                    ForEach(library.homeBoard.shelves) { shelf in
                        ShelfRow(shelf: shelf, width: width - 32, model: model)
                    }
                    ForEach(library.catalogBoard.shelves) { shelf in
                        ShelfRow(shelf: shelf, width: width - 32, model: model)
                    }
                }
                .padding(.horizontal, 16)
                .padding(.top, 12)
                .padding(.bottom, 24)
            }
            .refreshable { await library.refreshHome() }
            .tint(Palette.accent)
        }
    }

    private var emptyNote: String {
        if !library.servers.enabled.isEmpty { return "Waiting for your media servers…" }
        #if ADDONS
        return "Your add-ons have no catalogues to show here."
        #else
        return "Nothing to show here yet."
        #endif
    }

    private var empty: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                PageTitle(text: "Home")
                Text(Self.emptyText)
                    .font(Palette.font(14))
                    .foregroundStyle(Palette.textDim)
                PrimaryButton(text: "Open Settings") { model.select(.settings) }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(32)
        }
    }

    #if ADDONS
    static let emptyText = "Home fills with shelves of films and series from your media servers, your add-ons and the TMDB catalogue. Add a media server or a TMDB key in Settings, or install an add-on."
    #else
    static let emptyText = "Home fills with shelves of films and series from your media servers and the TMDB catalogue. Add a media server or a TMDB key in Settings."
    #endif
}

/// The banner: a few titles of the first shelf in turn, large, every 9 s
/// while Home shows, with dots for which one shows.
private struct HeroBanner: View {
    let slides: [HomeTop.Slide]
    let width: CGFloat
    let showing: Bool
    let pause: Pause
    let firstPicture: () -> Void
    let open: (HomeTop.Slide) -> Void
    @State private var index = 0

    var body: some View {
        let height = min(max(width * 1.2, 420), 560)
        VStack(spacing: 14) {
            let slide = slides[min(index, slides.count - 1)]
            Button { open(slide) } label: {
                ZStack(alignment: .bottom) {
                    RoundedRectangle(cornerRadius: 24).fill(Palette.surface)
                    RemotePicture(url: slide.image, size: .background, pause: pause,
                                  loaded: { if index == 0 { firstPicture() } }) { Color.clear }
                        .frame(width: width, height: height)
                        .clipped()
                    LinearGradient(stops: [.init(color: .clear, location: 0.35),
                                           .init(color: Color(red: 0x0d / 255, green: 0x0f / 255, blue: 0x14 / 255).opacity(0.9), location: 1)],
                                   startPoint: .top, endPoint: .bottom)
                    LinearGradient(stops: [.init(color: Color(red: 0x0d / 255, green: 0x0f / 255, blue: 0x14 / 255).opacity(0.7), location: 0),
                                           .init(color: .clear, location: 0.6)],
                                   startPoint: .leading, endPoint: .trailing)
                    VStack(spacing: 10) {
                        if let logo = slide.logo {
                            RemotePicture(url: logo, size: .logo, pause: pause) { titleText(slide) }
                                .scaledToFit()
                                .frame(maxWidth: min(width - 40, 320), maxHeight: 96)
                        } else {
                            titleText(slide)
                        }
                        if !slide.facts.isEmpty {
                            Text(slide.facts)
                                .font(Palette.font(13, .semibold))
                                .foregroundStyle(Color(white: 0.95))
                                .multilineTextAlignment(.center)
                        }
                        if !slide.scores.isEmpty {
                            HStack(spacing: 8) {
                                ForEach(slide.scores) { ScoreTag(pill: $0) }
                            }
                        }
                        if !slide.description.isEmpty {
                            Text(slide.description)
                                .font(Palette.font(13))
                                .foregroundStyle(Color(white: 0.84))
                                .multilineTextAlignment(.center)
                                .lineLimit(3)
                        }
                        HStack(spacing: 8) {
                            HavenIcon(name: "info", size: 18, tint: Palette.onAccent)
                            Text("Info").font(Palette.font(14, .bold)).foregroundStyle(Palette.onAccent)
                        }
                        .padding(.horizontal, 30)
                        .frame(height: 42)
                        .background(Capsule().fill(Palette.accent))
                        .padding(.top, 6)
                        .accessibilityLabel("Info about \(slide.title)")
                    }
                    .padding(.horizontal, 20)
                    .padding(.bottom, 22)
                }
                .frame(width: width, height: height)
                .clipShape(RoundedRectangle(cornerRadius: 24))
                .overlay(RoundedRectangle(cornerRadius: 24).strokeBorder(Palette.border, lineWidth: 1))
            }
            .buttonStyle(PressedStyle())
            .animation(.easeInOut(duration: 0.3), value: index)
            HStack(spacing: 8) {
                ForEach(slides.indices, id: \.self) { at in
                    Capsule()
                        .fill(at == index ? Palette.text : Palette.textDim.opacity(0.5))
                        .frame(width: at == index ? 30 : 8, height: 8)
                        .onTapGesture { index = at }
                        .animation(.easeInOut(duration: 0.2), value: index)
                }
            }
            .frame(maxWidth: .infinity)
        }
        .task(id: "\(showing)|\(slides.count)") {
            guard showing, slides.count > 1 else { return }
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(HomeTop.turnEvery))
                if Task.isCancelled { return }
                index = (index + 1) % slides.count
            }
        }
    }

    private func titleText(_ slide: HomeTop.Slide) -> some View {
        Text(slide.title)
            .font(Palette.font(30, .heavy))
            .foregroundStyle(.white)
            .multilineTextAlignment(.center)
            .lineLimit(2)
    }
}

/// "IMDb 8.4" in a dark pill.
private struct ScoreTag: View {
    let pill: HomeTop.ScorePill

    var body: some View {
        HStack(spacing: 6) {
            Text(pill.label).font(Palette.font(13, .heavy)).foregroundStyle(tone)
            Text(pill.value).font(Palette.font(14, .bold)).foregroundStyle(.white)
        }
        .padding(.horizontal, 10)
        .frame(height: 30)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color(red: 0x0b / 255, green: 0x0d / 255, blue: 0x12 / 255).opacity(0.72)))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color.white.opacity(0.12), lineWidth: 1))
    }

    private var tone: Color {
        switch pill.tone {
        case 0: Color(red: 0xf5 / 255, green: 0xc5 / 255, blue: 0x18 / 255)
        case 1: Color(red: 0x01 / 255, green: 0xb4 / 255, blue: 0xe4 / 255)
        case 2: Color(red: 0xfa / 255, green: 0x32 / 255, blue: 0x0a / 255)
        default: Color(red: 0x66 / 255, green: 0xcc / 255, blue: 0x33 / 255)
        }
    }
}

/// "Today", "In 4 days": gold within a week.
private struct WhenPill: View {
    let text: String
    let soon: Bool

    var body: some View {
        HStack {
            Spacer()
            Text(text)
                .font(Palette.font(11, .bold))
                .foregroundStyle(soon ? Palette.onAccent : .white)
                .padding(.horizontal, 7)
                .frame(height: 24)
                .background(Capsule().fill(soon ? Palette.accent : Color.black.opacity(0.63)))
        }
        .padding(.trailing, 12)
        .padding(.bottom, 8)
    }
}

/// How many episodes are left, in the top corner.
private struct LeftPill: View {
    let count: Int

    var body: some View {
        VStack {
            HStack {
                Spacer()
                Text("\(count)")
                    .font(Palette.font(13, .heavy))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 8)
                    .frame(minWidth: 28, minHeight: 28)
                    .background(Capsule().fill(Color.black.opacity(0.7)))
                    .accessibilityLabel(count == 1 ? "1 episode left" : "\(count) episodes left")
            }
            Spacer()
        }
        .padding(12)
    }
}

/// A shelf: its heading and source, then its posters sideways (or why it
/// has none).
struct ShelfRow: View {
    let shelf: Shelf
    let width: CGFloat
    let model: AppModel
    var counted = true

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text(shelf.title)
                    .font(Palette.heading(18))
                    .foregroundStyle(Palette.text)
                Text(shelf.source)
                    .font(Palette.font(12))
                    .foregroundStyle(Palette.textDim)
            }
            if shelf.loading && shelf.items.isEmpty {
                Text("Loading…")
                    .font(Palette.font(14))
                    .foregroundStyle(Palette.textDim)
                    .frame(height: 40)
            } else if !shelf.error.isEmpty && shelf.items.isEmpty {
                Text(shelf.error)
                    .font(Palette.font(14))
                    .foregroundStyle(Palette.textDim)
            } else if shelf.items.isEmpty {
                Text("Nothing here.")
                    .font(Palette.font(14))
                    .foregroundStyle(Palette.textDim)
            } else {
                PosterRow(items: shelf.items, width: width, model: model, pause: counted ? model.library.homePause : nil)
            }
        }
    }
}

/// Posters sideways, 3.15 across a phone.
struct PosterRow: View {
    let items: [MetaPreview]
    let width: CGFloat
    let model: AppModel
    var pause: Pause?

    var body: some View {
        let card = max((width - 24) / 3.15, 1)
        ScrollView(.horizontal, showsIndicators: false) {
            LazyHStack(spacing: 10) {
                ForEach(items, id: \.id) { meta in
                    Button {
                        model.open(Opening(id: meta.id, kind: meta.kind, title: meta.name, poster: meta.poster))
                    } label: {
                        PosterCard(id: meta.id, title: meta.name, poster: meta.poster, rating: meta.rating,
                                   watched: model.library.filmWatched(kind: meta.kind, id: meta.id), width: card, pause: pause)
                    }
                    .buttonStyle(PressedStyle())
                }
            }
        }
        .frame(height: PosterCard.height(card))
        .scrollClipDisabled()
    }
}
