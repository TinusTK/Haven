import SwiftUI

/// The Library: videos on this iPhone (Local), Downloads, the Watchlist and
/// History, under one switch.
struct LibraryPage: View {
    let model: AppModel
    var library: Library { model.library }

    static let tabs = ["Local", "Downloads", "Watchlist", "History"]

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            PageTitle(text: "Library")
                .padding(.horizontal, 16)
            Segments(labels: Self.tabs, current: Binding(get: { model.libraryTab }, set: { model.libraryTab = $0 }))
                .padding(.horizontal, 16)
            switch model.libraryTab {
            case 0: LocalList(model: model)
            case 1: DownloadList(model: model, downloads: library.downloads)
            case 2: PosterGrid(model: model, items: library.watchlist.map {
                    GridItemInfo(id: $0.titleID, kind: $0.kind, title: $0.title, note: $0.year, poster: $0.poster, video: nil, progress: 0, watched: false)
                }, empty: "Nothing saved yet. Open a film or series and choose Add to Watchlist.", removable: false)
            default: PosterGrid(model: model, items: library.history.map {
                    GridItemInfo(id: $0.titleID, kind: $0.kind, title: $0.title, note: DownloadRules.historyNote($0), poster: $0.poster,
                                 video: $0.videoID, progress: $0.finished ? 1 : max(Resume.share(position: $0.position, duration: $0.duration), 0.02),
                                 watched: $0.finished)
                }, empty: "Nothing watched yet. Films and episodes you play show here, newest first.", removable: true)
            }
        }
        .padding(.top, 12)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .onAppear { if model.libraryTab == 0 { library.lookForVideos() } }
        .onChange(of: model.libraryTab) { _, tab in if tab == 0 { library.lookForVideos() } }
    }
}

/// A row of chips, one chosen.
struct Segments: View {
    let labels: [String]
    @Binding var current: Int

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(Array(labels.enumerated()), id: \.offset) { index, label in
                    Chip(label: label, selected: current == index) { current = index }
                }
            }
        }
    }
}

/// Local: videos in Files › On My iPhone › Haven › Videos.
private struct LocalList: View {
    let model: AppModel
    var library: Library { model.library }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .top, spacing: 10) {
                Text(DownloadRules.localPlace + (library.localStatus.isEmpty ? "" : "  ·  " + library.localStatus))
                    .font(Palette.font(12))
                    .foregroundStyle(Palette.textDim)
                    .frame(maxWidth: .infinity, alignment: .leading)
                Chip(label: library.localBusy ? "Looking…" : "Look again") { library.lookForVideos() }
                    .disabled(library.localBusy)
            }
            .padding(.horizontal, 16)
            ScrollView {
                LazyVStack(spacing: 8) {
                    ForEach(library.localVideos) { found in
                        Button { library.playLocal(found) } label: {
                            HStack(spacing: 12) {
                                HavenIcon(name: "play", size: 18, tint: Palette.accent)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(found.name).font(Palette.font(14, .semibold)).foregroundStyle(Palette.text).lineLimit(1)
                                    Text(found.detail).font(Palette.font(12)).foregroundStyle(Palette.textDim).lineLimit(1)
                                }
                                Spacer(minLength: 0)
                            }
                            .padding(.horizontal, 14)
                            .frame(height: 56)
                            .background(RoundedRectangle(cornerRadius: 10).fill(Palette.surface))
                            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Palette.border, lineWidth: 1))
                        }
                        .buttonStyle(PressedStyle())
                        .accessibilityLabel("Play \(found.name)")
                    }
                }
                .padding(.horizontal, 16)
                .padding(.bottom, 16)
            }
            .refreshable { library.lookForVideos() }
            .tint(Palette.accent)
        }
    }
}

/// Downloads: films, then series by name.
private struct DownloadList: View {
    let model: AppModel
    let downloads: Downloads

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                HavenIcon(name: "folder", size: 18, tint: Palette.textDim)
                Text("Saved in \(DownloadRules.placeWords)").font(Palette.font(12)).foregroundStyle(Palette.textDim).lineLimit(1)
            }
            .padding(.horizontal, 16)
            if downloads.rows.isEmpty {
                Text(DownloadRules.nothingDownloaded)
                    .font(Palette.font(14))
                    .foregroundStyle(Palette.textDim)
                    .padding(.horizontal, 16)
                Spacer(minLength: 0)
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 8) {
                        ForEach(downloads.rows) { row in
                            if !row.section.isEmpty {
                                Text(row.section).font(Palette.heading(18)).foregroundStyle(Palette.text).padding(.top, 6)
                            }
                            if !row.group.isEmpty {
                                Text(row.group).font(Palette.font(13, .bold)).foregroundStyle(Palette.accent)
                            }
                            DownloadItem(row: row) {
                                if let request = downloads.playRequest(id: row.id) { model.library.play?(request) }
                            } toggle: {
                                downloads.toggle(row.id)
                            } remove: {
                                downloads.remove(row.id)
                            }
                        }
                    }
                    .padding(.horizontal, 16)
                    .padding(.bottom, 16)
                }
            }
        }
    }
}

/// One download: its name, how far it is, and play / pause or carry on / delete.
private struct DownloadItem: View {
    let row: DownloadRow
    let play: () -> Void
    let toggle: () -> Void
    let remove: () -> Void
    @State private var asking = false

    var body: some View {
        HStack(spacing: 10) {
            VStack(alignment: .leading, spacing: 5) {
                Text(row.title).font(Palette.font(14, .semibold)).foregroundStyle(Palette.text).lineLimit(1)
                Text(row.status)
                    .font(Palette.font(12))
                    .foregroundStyle(row.failed ? Palette.alert : Palette.textDim)
                    .lineLimit(1)
                if !row.done { ProgressBar(share: row.progress, track: Palette.border) }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if row.done {
                round("play", name: "Play \(row.title)", action: play)
            } else {
                round(row.running ? "pause" : "download", name: row.running ? "Pause" : "Carry on", action: toggle)
            }
            round("trash", name: "Delete \(row.title)") { asking = true }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .background(RoundedRectangle(cornerRadius: 10).fill(Palette.surface))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Palette.border, lineWidth: 1))
        .confirmationDialog("Delete \(row.title)?", isPresented: $asking, titleVisibility: .visible) {
            Button("Delete the download", role: .destructive, action: remove)
        } message: {
            Text("Its file is deleted from this iPhone.")
        }
    }

    private func round(_ icon: String, name: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HavenIcon(name: icon, size: 17, tint: Palette.text)
                .frame(width: 38, height: 38)
                .background(Circle().fill(Palette.selected))
        }
        .buttonStyle(PressedStyle())
        .accessibilityLabel(name)
    }
}

/// What a Watchlist or History poster shows.
struct GridItemInfo: Identifiable {
    var id: String
    var kind: String
    var title: String
    /// The year, or for History the episode or how much is left.
    var note: String
    var poster: String?
    var video: String?
    var progress: Double
    var watched: Bool
}

/// Posters three across; History's can be taken off with a long press.
private struct PosterGrid: View {
    let model: AppModel
    let items: [GridItemInfo]
    let empty: String
    let removable: Bool

    var body: some View {
        if items.isEmpty {
            Text(empty)
                .font(Palette.font(14))
                .foregroundStyle(Palette.textDim)
                .padding(.horizontal, 16)
            Spacer(minLength: 0)
        } else {
            GeometryReader { geometry in
                let card = max((geometry.size.width - 32 - 20) / 3, 1)
                ScrollView {
                    LazyVGrid(columns: Array(repeating: GridItem(.fixed(card), spacing: 10, alignment: .top), count: 3),
                              alignment: .leading, spacing: 14) {
                        ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                            Button {
                                model.open(Opening(id: item.id, kind: item.kind, title: item.title, video: item.video, poster: item.poster))
                            } label: {
                                VStack(alignment: .leading, spacing: 2) {
                                    PosterCard(id: item.id, title: item.title, poster: item.poster,
                                               watched: item.watched || model.library.filmWatched(kind: item.kind, id: item.id),
                                               progress: item.watched ? 0 : item.progress, width: card)
                                    if !item.note.isEmpty {
                                        Text(item.note).font(Palette.font(12)).foregroundStyle(Palette.textDim).lineLimit(1)
                                            .frame(width: card, alignment: .leading)
                                    }
                                }
                            }
                            .buttonStyle(PressedStyle())
                            .contextMenu {
                                if removable {
                                    Button("Remove from History", role: .destructive) {
                                        model.library.removeContinue(kind: item.kind, id: item.id)
                                    }
                                }
                            }
                        }
                    }
                    .padding(.horizontal, 16)
                    .padding(.bottom, 16)
                }
            }
        }
    }
}
