import SwiftUI

/// A title's page on the television: the wide picture behind, the details
/// on the left, and on the right (once asked for) the episodes, then the
/// places to watch. Menu closes the list, then the page.
struct TVTitlePage: View {
    let model: AppModel
    let page: TitlePage

    var body: some View {
        ZStack(alignment: .topLeading) {
            Palette.background
            RemotePicture(url: page.background, size: .background) { Color.clear }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .clipped()
            LinearGradient(stops: [.init(color: Palette.background.opacity(0.96), location: 0),
                                   .init(color: Palette.background.opacity(0.7), location: 0.5),
                                   .init(color: Palette.background.opacity(page.panelOpen ? 0.85 : 0.3), location: 1)],
                           startPoint: .leading, endPoint: .trailing)
            HStack(alignment: .top, spacing: 60) {
                TVDetails(model: model, page: page)
                    .frame(width: page.panelOpen ? 760 : 1100, alignment: .leading)
                    .focusSection()
                if page.panelOpen {
                    Group {
                        if page.sourcesShown {
                            TVSources(page: page, sources: page.sources)
                        } else {
                            TVEpisodes(page: page)
                        }
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                    .focusSection()
                    .transition(.move(edge: .trailing).combined(with: .opacity))
                }
            }
            .padding(.horizontal, TV.side)
            .padding(.vertical, 60)
        }
        .ignoresSafeArea()
        .animation(.easeOut(duration: 0.2), value: page.panelOpen)
        .onExitCommand {
            if page.panelOpen && page.sourcesShown && page.isSeries {
                page.episodeOpen = false
            } else if page.panelOpen {
                page.panelOpen = false
                page.episodeOpen = false
            } else {
                model.closeTitle()
            }
        }
    }
}

private struct TVDetails: View {
    let model: AppModel
    let page: TitlePage

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                if page.poster != nil && !page.panelOpen {
                    RemotePicture(url: page.poster, size: .poster) { Palette.surface }
                        .frame(width: 200, height: 300)
                        .clipShape(RoundedRectangle(cornerRadius: 14))
                        .accessibilityHidden(true)
                }
                Text(page.title)
                    .font(Palette.heading(56))
                    .foregroundStyle(Palette.text)
                    .fixedSize(horizontal: false, vertical: true)
                if page.episodeOpen && !page.episodeLabel.isEmpty {
                    Text(page.episodeLabel).font(Palette.font(30, .semibold)).foregroundStyle(Palette.accent)
                }
                if !page.facts.isEmpty {
                    Text(page.facts).font(Palette.font(26, .semibold)).foregroundStyle(Palette.text)
                }
                if page.loading { TVNote(text: "Loading details…") }
                if !page.error.isEmpty { TVNote(text: page.error) }
                if !page.description.isEmpty {
                    Text(page.description)
                        .font(Palette.font(26))
                        .foregroundStyle(Palette.text)
                        .lineLimit(page.panelOpen ? 5 : 8)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if !page.offers.isEmpty {
                    Text("Where to watch: " + page.offers.map(\.name).joined(separator: ", "))
                        .font(Palette.font(22))
                        .foregroundStyle(Palette.textDim)
                        .lineLimit(2)
                } else if !page.offersNote.isEmpty && !page.loading {
                    TVNote(text: page.offersNote)
                }
                // The buttons: the remote starts on the first.
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 24) {
                        if !page.isSeries && !page.loading {
                            TVChip(label: "Play", selected: true, icon: "play") { page.showFilmSources() }
                        }
                        if page.isSeries {
                            TVChip(label: "Episodes", selected: page.seasons.isEmpty) { page.panelOpen = true }
                        }
                        TVChip(label: page.saved ? "On Watchlist" : "Add to Watchlist", icon: page.saved ? "check" : "plus") {
                            page.toggleWatchlist()
                        }
                        if page.film || page.watched {
                            TVChip(label: page.watched ? "Watched" : "Mark watched", icon: page.watched ? "check" : nil) {
                                page.markFilm(!page.watched)
                            }
                        }
                    }
                    .padding(.vertical, 16)
                    .padding(.horizontal, 6)
                }
                .scrollClipDisabled()
                if !page.seasons.isEmpty && !page.panelOpen {
                    TVSeasons(page: page, inPanel: false)
                }
            }
            .padding(.top, 20)
        }
        .scrollClipDisabled()
    }
}

/// The seasons; choosing one lists its episodes.
private struct TVSeasons: View {
    let page: TitlePage
    let inPanel: Bool

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 18) {
                ForEach(page.seasons, id: \.self) { number in
                    TVChip(label: number == 0 ? "Specials" : "Season \(number)", selected: inPanel && number == page.season) {
                        if inPanel { page.showSeason(number) } else { page.chooseSeason(number) }
                    }
                }
            }
            .padding(.vertical, 14)
            .padding(.horizontal, 6)
        }
        .scrollClipDisabled()
        .focusSection()
    }
}

private struct TVEpisodes: View {
    @Bindable var page: TitlePage

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            TVHeading(text: page.seasons.isEmpty ? "Episodes" : page.season == 0 ? "Specials" : "Season \(page.season)")
            if !page.seasons.isEmpty {
                TVSeasons(page: page, inPanel: true)
                if !page.episodes.isEmpty {
                    TVChip(label: page.seasonAllWatched ? "Season watched · mark unwatched" : "Mark season watched",
                           selected: page.seasonAllWatched) {
                        page.markSeason(!page.seasonAllWatched)
                    }
                }
            }
            if page.episodes.isEmpty { TVNote(text: "No episodes.") }
            ScrollView {
                LazyVStack(spacing: 14) {
                    ForEach(Array(page.episodes.enumerated()), id: \.offset) { _, episode in
                        TVRow(selected: episode.selected) {
                            page.openEpisode(episode.id)
                        } content: {
                            Text(episode.number)
                                .font(Palette.font(24, .semibold))
                                .foregroundStyle(Palette.accent)
                                .frame(width: 90, alignment: .leading)
                            Text(episode.title).font(Palette.font(26)).foregroundStyle(Palette.text).lineLimit(1)
                            Spacer(minLength: 0)
                            Text(episode.date).font(Palette.font(22)).foregroundStyle(Palette.textDim)
                            if episode.watched { WatchedTick(size: 34) }
                        }
                        .contextMenu {
                            Button(episode.watched ? "Mark unwatched" : "Mark watched") {
                                page.markEpisodes([episode.id], on: !episode.watched)
                            }
                        }
                    }
                }
                .padding(.vertical, 12)
                .padding(.horizontal, 8)
            }
            .scrollClipDisabled()
            TVNote(text: "Hold Select on an episode to mark it watched.")
        }
    }
}

private struct TVSources: View {
    let page: TitlePage
    @Bindable var sources: SourcesPanel

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            TVHeading(text: page.isSeries && !page.episodeLabel.isEmpty ? page.episodeLabel : TitleRules.heading)
            if !page.playNote.isEmpty { TVNote(text: page.playNote) }
            if !page.resumeNote.isEmpty {
                Text(page.resumeNote).font(Palette.font(24)).foregroundStyle(Palette.accent)
            }
            if sources.groups.count > 1 {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 18) {
                        TVChip(label: "All  \(sources.rows.count)", selected: sources.group.isEmpty) { sources.group = "" }
                        ForEach(sources.groups, id: \.name) { group in
                            TVChip(label: "\(group.name)  \(group.count)", selected: sources.group == group.name) {
                                sources.group = group.name
                            }
                        }
                    }
                    .padding(.vertical, 14)
                    .padding(.horizontal, 6)
                }
                .scrollClipDisabled()
                .focusSection()
            }
            if sources.loading {
                TVNote(text: sources.rows.isEmpty ? TitleRules.asking : "More are still coming in…")
            }
            if !sources.note.isEmpty { TVNote(text: sources.note) }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 14) {
                    ForEach(sources.shownRows) { row in
                        if !row.heading.isEmpty {
                            Text(row.heading).font(Palette.font(24, .bold)).foregroundStyle(Palette.accent).padding(.top, 8)
                        }
                        TVRow {
                            page.play(row)
                        } content: {
                            VStack(alignment: .leading, spacing: 6) {
                                HStack(spacing: 12) {
                                    Text(row.name).font(Palette.font(26, .bold)).foregroundStyle(Palette.text).lineLimit(2)
                                    if !row.size.isEmpty {
                                        Text(row.size)
                                            .font(Palette.font(20, .bold))
                                            .foregroundStyle(Palette.accent)
                                            .padding(.horizontal, 12)
                                            .frame(height: 32)
                                            .background(Capsule().fill(Palette.accent.opacity(0.15)))
                                    }
                                }
                                if !row.detail.isEmpty {
                                    Text(row.detail).font(Palette.font(22)).foregroundStyle(Palette.textDim).lineLimit(2)
                                }
                                if !row.detail2.isEmpty {
                                    Text(row.detail2).font(Palette.font(22)).foregroundStyle(Palette.textDim).lineLimit(2)
                                }
                                Text(row.group == row.source ? row.source : "\(row.group)  ·  \(row.source)")
                                    .font(Palette.font(20))
                                    .foregroundStyle(Palette.textDim)
                                    .lineLimit(1)
                            }
                            Spacer(minLength: 0)
                            if row.playable {
                                HavenIcon(name: "play", size: 30, tint: Palette.accent)
                            } else {
                                Text(row.kind).font(Palette.font(22)).foregroundStyle(Palette.textDim)
                            }
                        }
                        .disabled(!row.playable)
                        if row.more > 0 {
                            TVChip(label: "Show more from \(row.group)  (\(row.more) left)") { sources.showMore(row.group) }
                        }
                    }
                }
                .padding(.vertical, 12)
                .padding(.horizontal, 8)
            }
            .scrollClipDisabled()
        }
    }
}
