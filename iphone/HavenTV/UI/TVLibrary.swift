import SwiftUI

/// The Library on the television: the Watchlist and History, as on the
/// iPhone. (No Local or Downloads: Apple TV keeps no videos of its own.)
struct TVLibraryPage: View {
    let model: AppModel
    var library: Library { model.library }

    static let tabs = ["Watchlist", "History"]

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 40) {
                HStack(spacing: 20) {
                    ForEach(Array(Self.tabs.enumerated()), id: \.offset) { index, label in
                        TVChip(label: label, selected: tab == index) { tab = index }
                    }
                }
                .padding(.vertical, 14)
                .padding(.horizontal, 6)
                .focusSection()
                if tab == 0 {
                    TVPosterGrid(model: model, items: library.watchlist.map {
                        GridItemInfo(id: $0.titleID, kind: $0.kind, title: $0.title, note: $0.year, poster: $0.poster,
                                     video: nil, progress: 0, watched: false)
                    }, empty: "Nothing saved yet. Open a film or series and choose Add to Watchlist.", removable: false)
                } else {
                    TVPosterGrid(model: model, items: library.history.map {
                        GridItemInfo(id: $0.titleID, kind: $0.kind, title: $0.title, note: DownloadRules.historyNote($0), poster: $0.poster,
                                     video: $0.videoID,
                                     progress: $0.finished ? 1 : max(Resume.share(position: $0.position, duration: $0.duration), 0.02),
                                     watched: $0.finished)
                    }, empty: "Nothing watched yet. Films and episodes you play show here, newest first.", removable: true)
                }
            }
            .padding(.horizontal, TV.side)
            .padding(.vertical, 40)
        }
    }

    /// Watchlist or History; the iPhone's Local and Downloads (0 and 1)
    /// show the Watchlist here.
    private var tab: Int {
        get { model.libraryTab == 3 ? 1 : 0 }
        nonmutating set { model.libraryTab = newValue == 1 ? 3 : 2 }
    }
}

/// What a Watchlist or History poster shows (as on the iPhone).
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

/// Posters six across; History's come off with a long press on Select.
private struct TVPosterGrid: View {
    let model: AppModel
    let items: [GridItemInfo]
    let empty: String
    let removable: Bool

    var body: some View {
        if items.isEmpty {
            TVNote(text: empty)
        } else {
            LazyVGrid(columns: Array(repeating: GridItem(.fixed(TV.poster), spacing: 48, alignment: .top), count: 6),
                      alignment: .leading, spacing: 56) {
                ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                    Button {
                        model.open(Opening(id: item.id, kind: item.kind, title: item.title, video: item.video, poster: item.poster))
                    } label: {
                        VStack(alignment: .leading, spacing: 4) {
                            PosterCard(id: item.id, title: item.title, poster: item.poster,
                                       watched: item.watched || model.library.filmWatched(kind: item.kind, id: item.id),
                                       progress: item.watched ? 0 : item.progress, width: TV.poster)
                            if !item.note.isEmpty {
                                Text(item.note).font(Palette.font(20)).foregroundStyle(Palette.textDim).lineLimit(1)
                                    .frame(width: TV.poster, alignment: .leading)
                            }
                        }
                    }
                    .buttonStyle(TVPosterStyle())
                    .contextMenu {
                        if removable {
                            Button("Remove from History", role: .destructive) {
                                model.library.removeContinue(kind: item.kind, id: item.id)
                            }
                        }
                    }
                }
            }
            .focusSection()
        }
    }
}
