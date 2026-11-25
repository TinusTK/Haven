import SwiftUI

/// A title's page: the wide picture behind, the details over it, and under
/// them (once asked for) a panel with the episodes, then the places to
/// watch. On a phone the details take the top part while the panel is open.
struct TitlePageView: View {
    let model: AppModel
    let page: TitlePage

    var body: some View {
        GeometryReader { geometry in
            let top = page.panelOpen ? min(geometry.size.height * 0.4, 330) : geometry.size.height
            ZStack(alignment: .topLeading) {
                Palette.background
                RemotePicture(url: page.background, size: .background) { Color.clear }
                    .frame(width: geometry.size.width, height: geometry.size.height)
                    .clipped()
                LinearGradient(stops: [.init(color: Palette.background.opacity(0.94), location: 0),
                                       .init(color: Palette.background.opacity(0.6), location: 0.55),
                                       .init(color: Palette.background.opacity(0.4), location: 1)],
                               startPoint: .leading, endPoint: .trailing)
                LinearGradient(stops: [.init(color: .clear, location: 0.35),
                                       .init(color: Palette.background.opacity(0.9), location: 1)],
                               startPoint: .top, endPoint: .bottom)
                Details(model: model, page: page, height: top)
                    .frame(width: geometry.size.width, height: top)
                if page.panelOpen {
                    Panel(page: page)
                        .frame(width: geometry.size.width - 24, height: max(geometry.size.height - top - 10, 0))
                        .background(RoundedRectangle(cornerRadius: 18).fill(Palette.surface.opacity(0.96)))
                        .overlay(RoundedRectangle(cornerRadius: 18).strokeBorder(Palette.border, lineWidth: 1))
                        .clipShape(RoundedRectangle(cornerRadius: 18))
                        .offset(x: 12, y: top)
                        .transition(.move(edge: .bottom).combined(with: .opacity))
                }
            }
            .animation(.easeOut(duration: 0.2), value: page.panelOpen)
        }
        .background(Palette.background)
    }
}

/// The title, on the left over the picture; scrolls when it does not fit.
private struct Details: View {
    let model: AppModel
    let page: TitlePage
    let height: CGFloat

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                Button { model.closeTitle() } label: {
                    HStack(spacing: 4) {
                        HavenIcon(name: "back", size: 16, tint: Palette.text)
                        Text("Back").font(Palette.font(14, .semibold)).foregroundStyle(Palette.text)
                    }
                    .padding(.horizontal, 12)
                    .frame(height: 36)
                    .background(Capsule().fill(Palette.surface.opacity(0.9)))
                    .overlay(Capsule().strokeBorder(Palette.border, lineWidth: 1))
                }
                .buttonStyle(PressedStyle())
                .accessibilityLabel("Back")
                Spacer(minLength: 0)
                if !page.panelOpen && height > 520, page.poster != nil {
                    RemotePicture(url: page.poster, size: .poster) { Palette.surface }
                        .frame(width: 110, height: 165)
                        .clipShape(RoundedRectangle(cornerRadius: 10))
                        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Palette.border, lineWidth: 1))
                        .accessibilityHidden(true)
                }
                Text(page.title)
                    .font(Palette.heading(24))
                    .foregroundStyle(Palette.text)
                    .fixedSize(horizontal: false, vertical: true)
                if page.film || page.watched { FilmMark(page: page) }
                if page.episodeOpen && !page.episodeLabel.isEmpty {
                    Text(page.episodeLabel)
                        .font(Palette.font(18, .semibold))
                        .foregroundStyle(Palette.accent)
                }
                if !page.facts.isEmpty {
                    Text(page.facts)
                        .font(Palette.font(14, .semibold))
                        .foregroundStyle(Palette.text)
                }
                if page.loading {
                    Text("Loading details…").font(Palette.font(14)).foregroundStyle(Palette.textDim)
                }
                if !page.error.isEmpty {
                    Text(page.error).font(Palette.font(14)).foregroundStyle(Palette.textDim)
                }
                if !page.description.isEmpty {
                    Text(page.description)
                        .font(Palette.font(14))
                        .foregroundStyle(Palette.text)
                        .lineLimit(page.panelOpen ? 3 : 7)
                }
                WhereToWatchRow(page: page)
                HStack(spacing: 10) {
                    if !page.isSeries && !page.loading {
                        PrimaryButton(text: "▶  Sources") { page.showFilmSources() }
                    }
                    if page.isSeries && page.seasons.isEmpty {
                        PrimaryButton(text: "Episodes") { page.panelOpen = true }
                    }
                    Chip(label: page.saved ? "✓  On Watchlist" : "＋  Add to Watchlist", selected: page.saved) {
                        page.toggleWatchlist()
                    }
                }
                .padding(.top, 6)
                if !page.seasons.isEmpty && !page.panelOpen {
                    SeasonChips(page: page, selected: false)
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .frame(minHeight: height, alignment: .topLeading)
        }
        .scrollIndicators(.hidden)
    }
}

/// A film: the pill marks it watched or unwatched.
private struct FilmMark: View {
    let page: TitlePage

    var body: some View {
        Button { page.markFilm(!page.watched) } label: {
            HStack(spacing: 7) {
                if page.watched {
                    WatchedTick(size: 20)
                } else {
                    Circle().strokeBorder(Palette.textDim, lineWidth: 1.5).frame(width: 20, height: 20)
                }
                Text(page.watched ? "Watched" : "Mark watched")
                    .font(Palette.font(13, .bold))
                    .foregroundStyle(page.watched ? Palette.accent : Palette.text)
            }
            .padding(.leading, 6)
            .padding(.trailing, 14)
            .frame(height: 30)
            .background(Capsule().fill(page.watched ? Palette.accent.opacity(0.14) : Color.clear))
            .overlay(Capsule().strokeBorder(page.watched ? Palette.accent : Palette.textDim.opacity(0.6), lineWidth: 1.5))
        }
        .buttonStyle(PressedStyle())
        .accessibilityLabel(page.watched ? "Watched. Mark unwatched" : "Mark watched")
    }
}

/// Where to watch: the services that have it, grouped Stream / Free / Rent
/// / Buy, on Haven's own tiles. A tile opens JustWatch.
private struct WhereToWatchRow: View {
    let page: TitlePage

    var body: some View {
        if !page.offers.isEmpty {
            VStack(alignment: .leading, spacing: 6) {
                Text("Where to watch  ·  \(page.offersNote)")
                    .font(Palette.font(12))
                    .foregroundStyle(Palette.textDim)
                    .lineLimit(1)
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(alignment: .top, spacing: 8) {
                        ForEach(Array(page.offers.enumerated()), id: \.offset) { index, tile in
                            Button {
                                if !page.offersLink.isEmpty { Browser.open(page.offersLink) }
                            } label: {
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(tile.group)
                                        .font(Palette.font(11, .semibold))
                                        .foregroundStyle(Palette.accent)
                                        .frame(height: 14)
                                    Text(tile.letters)
                                        .font(Palette.font(tile.letters.count > 2 ? 13 : 17, .heavy))
                                        .foregroundStyle(Color(hex: tile.ink))
                                        .frame(width: 52, height: 52)
                                        .background(RoundedRectangle(cornerRadius: 12).fill(Color(hex: tile.tint)))
                                        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.white.opacity(0.08), lineWidth: 1))
                                    Text(tile.name)
                                        .font(Palette.font(11))
                                        .foregroundStyle(Palette.textDim)
                                        .lineLimit(1)
                                        .frame(width: 56, alignment: .leading)
                                }
                                .padding(.leading, !tile.group.isEmpty && index > 0 ? 16 : 0)
                            }
                            .buttonStyle(PressedStyle())
                            .accessibilityElement(children: .ignore)
                            .accessibilityLabel("\(tile.name), \(tile.ways). Opens JustWatch")
                            .accessibilityAddTraits(.isButton)
                        }
                    }
                }
                Text(WhereToWatch.creditNote)
                    .font(Palette.font(10))
                    .foregroundStyle(Palette.textDim.opacity(0.8))
            }
        } else if !page.offersNote.isEmpty && !page.loading {
            Text(page.offersNote)
                .font(Palette.font(12))
                .foregroundStyle(Palette.textDim)
        }
    }
}

/// The seasons; picking one lists its episodes in the panel.
private struct SeasonChips: View {
    let page: TitlePage
    /// Mark the season showing (inside the panel).
    let selected: Bool

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(page.seasons, id: \.self) { number in
                    Chip(label: number == 0 ? "Specials" : "Season \(number)", selected: selected && number == page.season) {
                        if selected { page.showSeason(number) } else { page.chooseSeason(number) }
                    }
                }
            }
        }
    }
}

/// The panel: a series' episodes, then (an episode picked, or a film) the
/// places to watch.
private struct Panel: View {
    let page: TitlePage

    var body: some View {
        if page.sourcesShown {
            SourcesList(page: page, sources: page.sources)
        } else {
            EpisodeList(page: page)
        }
    }
}

private struct PanelHeading<Leading: View>: View {
    let title: String
    let page: TitlePage
    @ViewBuilder var leading: () -> Leading

    var body: some View {
        HStack(spacing: 10) {
            leading()
            Text(title)
                .font(Palette.heading(16))
                .foregroundStyle(Palette.text)
                .lineLimit(1)
            Spacer(minLength: 0)
            Button {
                page.panelOpen = false
                page.episodeOpen = false
            } label: {
                HavenIcon(name: "close", size: 14, tint: Palette.textDim)
                    .frame(width: 32, height: 32)
                    .background(Circle().fill(Palette.background.opacity(0.5)))
            }
            .buttonStyle(PressedStyle())
            .accessibilityLabel("Close the list")
        }
    }
}

private struct EpisodeList: View {
    @Bindable var page: TitlePage

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            PanelHeading(title: page.seasons.isEmpty ? "Episodes" : page.season == 0 ? "Specials" : "Season \(page.season)",
                         page: page) { EmptyView() }
            if !page.seasons.isEmpty {
                SeasonChips(page: page, selected: true)
                if !page.episodes.isEmpty || !page.episodeFilter.isEmpty {
                    Chip(label: page.seasonAllWatched ? "✓  Season watched · mark unwatched" : "Mark season watched",
                         selected: page.seasonAllWatched) {
                        page.markSeason(!page.seasonAllWatched)
                    }
                }
                HavenField(placeholder: "Filter episodes", text: $page.episodeFilter)
            }
            if page.episodes.isEmpty {
                Text("No episode matches.").font(Palette.font(14)).foregroundStyle(Palette.textDim)
            }
            ScrollView {
                LazyVStack(spacing: 8) {
                    ForEach(Array(page.episodes.enumerated()), id: \.offset) { _, episode in
                        EpisodeItem(episode: episode) {
                            page.openEpisode(episode.id)
                        } toggle: {
                            page.markEpisodes([episode.id], on: !episode.watched)
                        }
                    }
                }
                .padding(.bottom, 12)
            }
            .scrollDismissesKeyboard(.immediately)
        }
        .padding(16)
    }
}

/// One episode: its number, name and date, and the round mark at the end.
private struct EpisodeItem: View {
    let episode: EpisodeRow
    let open: () -> Void
    let toggle: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            Button(action: open) {
                HStack(spacing: 12) {
                    Text(episode.number)
                        .font(Palette.font(14, .semibold))
                        .foregroundStyle(Palette.accent)
                        .frame(width: 44, alignment: .leading)
                    Text(episode.title)
                        .font(Palette.font(14))
                        .foregroundStyle(Palette.text)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                    Text(episode.date)
                        .font(Palette.font(12))
                        .foregroundStyle(Palette.textDim)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("\(episode.number) \(episode.title)")
            Button(action: toggle) {
                Group {
                    if episode.watched {
                        WatchedTick(size: 24)
                    } else {
                        Circle().strokeBorder(Palette.textDim.opacity(0.5), lineWidth: 1.5).frame(width: 24, height: 24)
                    }
                }
                .frame(width: 30, height: 44)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(episode.watched ? "Watched. Mark unwatched" : "Mark watched")
        }
        .padding(.horizontal, 14)
        .frame(height: 44)
        .background(RoundedRectangle(cornerRadius: 8).fill(episode.selected ? Palette.selected : Palette.surface))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Palette.border, lineWidth: 1))
    }
}

private struct SourcesList: View {
    let page: TitlePage
    @Bindable var sources: SourcesPanel

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            PanelHeading(title: page.isSeries && !page.episodeLabel.isEmpty ? page.episodeLabel : TitleRules.heading, page: page) {
                if page.isSeries {
                    Chip(label: "‹") { page.episodeOpen = false }
                        .accessibilityLabel("Back to the episodes")
                }
            }
            if !page.playNote.isEmpty {
                Text(page.playNote).font(Palette.font(13)).foregroundStyle(Palette.accent)
            }
            if !page.resumeNote.isEmpty {
                Text(page.resumeNote).font(Palette.font(13)).foregroundStyle(Palette.accent)
            }
            if sources.groups.count > 1 {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        Chip(label: "All  \(sources.rows.count)", selected: sources.group.isEmpty) { sources.group = "" }
                        ForEach(sources.groups, id: \.name) { group in
                            Chip(label: "\(group.name)  \(group.count)", selected: sources.group == group.name) {
                                sources.group = group.name
                            }
                        }
                    }
                }
            }
            if page.loading && !sources.loading {
                Text("Loading…").font(Palette.font(13)).foregroundStyle(Palette.textDim)
            }
            if sources.loading {
                Text(sources.rows.isEmpty ? TitleRules.asking : "More are still coming in…")
                    .font(Palette.font(13))
                    .foregroundStyle(Palette.textDim)
            }
            if !sources.note.isEmpty {
                Text(sources.note).font(Palette.font(14)).foregroundStyle(Palette.textDim)
            }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 8) {
                    ForEach(sources.shownRows) { row in
                        if !row.heading.isEmpty {
                            Text(row.heading)
                                .font(Palette.font(13, .bold))
                                .foregroundStyle(Palette.accent)
                                .padding(.top, 6)
                                .accessibilityAddTraits(.isHeader)
                        }
                        SourceCard(row: row) { page.play(row) }
                        if row.more > 0 {
                            Chip(label: "Show more from \(row.group)  (\(row.more) left)") { sources.showMore(row.group) }
                        }
                    }
                }
                .padding(.bottom, 8)
            }
            Rectangle().fill(Palette.border).frame(height: 1)
            HStack(spacing: 8) {
                Text("Play on").font(Palette.font(13)).foregroundStyle(Palette.textDim)
                Chip(label: "This iPhone", selected: true) {}
                    .allowsHitTesting(false)
                AirPlayButton()
                    .frame(width: 34, height: 34)
                    .accessibilityLabel("AirPlay")
                Spacer(minLength: 0)
            }
        }
        .padding(16)
    }
}

/// A place to watch: its name, size and how many share it, the details,
/// where it came from, and Play (or how it is delivered, when Haven
/// cannot play it).
private struct SourceCard: View {
    let row: StreamRow
    let play: () -> Void

    var body: some View {
        Button(action: play) {
            HStack(spacing: 12) {
                VStack(alignment: .leading, spacing: 3) {
                    HStack(alignment: .top, spacing: 8) {
                        Text(row.name)
                            .font(Palette.font(14, .bold))
                            .foregroundStyle(Palette.text)
                            .multilineTextAlignment(.leading)
                            .fixedSize(horizontal: false, vertical: true)
                        Spacer(minLength: 0)
                        if !row.size.isEmpty {
                            Text(row.size)
                                .font(Palette.font(11, .bold))
                                .foregroundStyle(Palette.accent)
                                .padding(.horizontal, 7)
                                .frame(height: 20)
                                .background(Capsule().fill(Palette.accent.opacity(0.15)))
                        }
                        if !row.peers.isEmpty {
                            HStack(spacing: 3) {
                                HavenIcon(name: "account", size: 12, tint: Palette.text, stroke: 2.2)
                                Text(row.peers).font(Palette.font(11, .bold)).foregroundStyle(Palette.text)
                            }
                            .padding(.horizontal, 7)
                            .frame(height: 20)
                            .background(Capsule().fill(Palette.textDim.opacity(0.15)))
                            .accessibilityElement(children: .ignore)
                            .accessibilityLabel("\(row.peers) sharing")
                        }
                    }
                    if !row.detail.isEmpty {
                        Text(row.detail).font(Palette.font(12)).foregroundStyle(Palette.textDim).multilineTextAlignment(.leading)
                    }
                    if !row.detail2.isEmpty {
                        Text(row.detail2).font(Palette.font(12)).foregroundStyle(Palette.textDim).multilineTextAlignment(.leading)
                    }
                    Text(row.group == row.source ? row.source : "\(row.group)  ·  \(row.source)")
                        .font(Palette.font(11))
                        .foregroundStyle(Palette.textDim)
                        .lineLimit(1)
                }
                if row.playable {
                    Text("▶\u{FE0E}")
                        .font(Palette.font(14))
                        .foregroundStyle(Palette.accent)
                        .frame(width: 38, height: 38)
                        .background(Circle().fill(Palette.selected))
                } else {
                    Text(row.kind).font(Palette.font(12)).foregroundStyle(Palette.textDim)
                }
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 11)
            .frame(minHeight: 88)
            .background(RoundedRectangle(cornerRadius: 12).fill(Palette.background.opacity(0.55)))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Palette.border, lineWidth: 1))
        }
        .buttonStyle(PressedStyle())
        .disabled(!row.playable)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(row.playable ? "Play " : "")\(row.name), \(row.kind), from \(row.source)")
    }
}

extension Color {
    /// A colour written as 0xRRGGBB.
    init(hex: UInt32) {
        self.init(red: Double((hex >> 16) & 0xff) / 255, green: Double((hex >> 8) & 0xff) / 255, blue: Double(hex & 0xff) / 255)
    }
}
