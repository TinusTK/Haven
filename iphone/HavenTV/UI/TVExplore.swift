import SwiftUI

/// Explore on the television: a search box over everything, and under it
/// one source at a time to browse (TMDB, a media server's library, a
/// catalogue) as a grid of posters.
struct TVExplorePage: View {
    let model: AppModel
    @Bindable var explore: Explore
    var library: Library { model.library }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 40) {
                HStack(spacing: 24) {
                    TextField("Search films and series", text: $explore.query)
                        .font(Palette.font(28))
                        .autocorrectionDisabled()
                        .onSubmit { run(explore.query) }
                        .frame(maxWidth: 900)
                    if explore.searching {
                        TVChip(label: "Clear the search") {
                            explore.query = ""
                            explore.searching = false
                            library.clearSearch()
                        }
                    }
                }
                .focusSection()
                if explore.searching {
                    if !library.searchNote.isEmpty { TVNote(text: library.searchNote) }
                    ForEach(library.searchBoard.shelves) { TVShelfRow(shelf: $0, model: model, counted: false) }
                } else if explore.sources.isEmpty {
                    TVNote(text: "Explore browses everything you can watch. Add a media server or a TMDB key in Settings and it shows here.")
                } else {
                    browse
                }
            }
            .padding(.horizontal, TV.side)
            .padding(.vertical, 40)
        }
        .onAppear { explore.appeared() }
    }

    private func run(_ text: String) {
        explore.searching = !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        library.runSearch(text)
    }

    /// The source, Films or Series, the genre, then the grid.
    private var browse: some View {
        VStack(alignment: .leading, spacing: 30) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 20) {
                    Menu {
                        ForEach(explore.rows.filter { !$0.header }, id: \.index) { row in
                            Button(row.label) { explore.chooseSource(row.index) }
                        }
                    } label: {
                        Text(sourceName).font(Palette.font(26, .semibold)).lineLimit(1)
                    }
                    ForEach(Array(explore.kinds.enumerated()), id: \.offset) { index, label in
                        TVChip(label: label, selected: explore.kind == index) { explore.chooseKind(index) }
                    }
                    if !explore.genres.isEmpty {
                        Menu {
                            ForEach(Array(explore.genres.enumerated()), id: \.offset) { choice, name in
                                Button(name) { explore.chooseGenre(choice) }
                            }
                        } label: {
                            Text(explore.genres.indices.contains(explore.genre + 1) ? explore.genres[explore.genre + 1] : "All genres")
                                .font(Palette.font(26, .semibold))
                        }
                    }
                }
                .padding(.vertical, 14)
                .padding(.horizontal, 6)
            }
            .scrollClipDisabled()
            .focusSection()
            if !explore.note.isEmpty || (explore.loading && explore.items.isEmpty) {
                TVNote(text: explore.note.isEmpty ? "Loading…" : explore.note)
            }
            LazyVGrid(columns: Array(repeating: GridItem(.fixed(TV.poster), spacing: 48, alignment: .top), count: 6),
                      alignment: .leading, spacing: 56) {
                ForEach(explore.items, id: \.self) { meta in
                    TVPoster(meta: meta, model: model)
                }
            }
            .focusSection()
            if explore.more && !explore.items.isEmpty {
                TVChip(label: explore.loading ? "Loading…" : "Load more") { explore.load() }
                    .disabled(explore.loading)
            }
        }
    }

    private var sourceName: String {
        explore.rows.first { !$0.header && $0.index == explore.source }?.label ?? "Choose a source"
    }
}
