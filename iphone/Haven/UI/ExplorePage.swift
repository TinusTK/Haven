import SwiftUI

/// Explore: a search box over everything, and a grid to browse one source
/// at a time (TMDB, a media server's library, a catalogue). On a phone the
/// list of sources opens from the "Sources ›" chip.
struct ExplorePage: View {
    let model: AppModel
    var library: Library { model.library }
    var explore: Explore { model.library.explore }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 14) {
                PageTitle(text: "Explore")
                SearchBox(explore: explore, search: { library.runSearch($0) }, clear: { library.clearSearch() })
            }
            .padding(.horizontal, 16)
            if explore.searching {
                results
            } else if explore.sources.isEmpty {
                Text(Self.emptyText)
                    .font(Palette.font(14))
                    .foregroundStyle(Palette.textDim)
                    .padding(.horizontal, 16)
                Spacer(minLength: 0)
            } else {
                Chip(label: explore.picking ? "‹  Back to the titles" : "Sources  ›", selected: explore.picking) {
                    explore.picking.toggle()
                }
                .padding(.horizontal, 16)
                if explore.picking {
                    SourceList(explore: explore)
                } else {
                    TitleGrid(model: model, explore: explore)
                }
            }
        }
        .padding(.top, 12)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .onAppear { explore.appeared() }
    }

    /// Search's shelves, each source's answer in a row.
    private var results: some View {
        GeometryReader { geometry in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 20) {
                    if !library.searchNote.isEmpty {
                        Text(library.searchNote)
                            .font(Palette.font(14))
                            .foregroundStyle(Palette.textDim)
                    }
                    ForEach(library.searchBoard.shelves) { shelf in
                        ShelfRow(shelf: shelf, width: geometry.size.width - 32, model: model, counted: false)
                    }
                }
                .padding(.horizontal, 16)
                .padding(.bottom, 16)
            }
            .scrollDismissesKeyboard(.immediately)
        }
    }

    #if ADDONS
    static let emptyText = "Explore browses everything you can watch. Add a media server or a TMDB key in Settings, or install an add-on, and it shows here."
    #else
    static let emptyText = "Explore browses everything you can watch. Add a media server or a TMDB key in Settings and it shows here."
    #endif
}

/// The round search box: Enter searches; with nothing typed, Paste pastes
/// and searches at once; ✕ clears the search.
private struct SearchBox: View {
    @Bindable var explore: Explore
    let search: (String) -> Void
    let clear: () -> Void

    @Environment(Typing.self) private var typing
    @FocusState private var focused: Bool
    @State private var frame: CGRect = .zero

    var body: some View {
        HStack(spacing: 8) {
            HavenIcon(name: "search", size: 18, tint: Palette.textDim)
            TextField("", text: $explore.query, prompt: Text("Search films and series").foregroundStyle(Palette.textDim))
                .font(Palette.font(14))
                .foregroundStyle(Palette.text)
                .tint(Palette.accent)
                .autocorrectionDisabled()
                .textInputAutocapitalization(.never)
                .submitLabel(.search)
                .focused($focused)
                .onSubmit { run(explore.query) }
                .accessibilityLabel("Search films and series")
            if explore.searching || !explore.query.isEmpty {
                Button {
                    explore.query = ""
                    explore.searching = false
                    clear()
                } label: {
                    HavenIcon(name: "close", size: 14, tint: Palette.textDim)
                        .frame(width: 28, height: 28)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Clear the search")
            } else {
                PasteButton(payloadType: String.self) { pasted in
                    guard let first = pasted.first else { return }
                    Task { @MainActor in
                        explore.query = first
                        run(first)
                    }
                }
                .labelStyle(.iconOnly)
                .buttonBorderShape(.capsule)
                .tint(Palette.selected)
                .foregroundStyle(Palette.text)
                .accessibilityLabel("Paste")
            }
        }
        .padding(.leading, 14)
        .padding(.trailing, 6)
        .frame(height: 40)
        .background(Capsule().fill(Palette.surface))
        .overlay(Capsule().strokeBorder(focused ? Palette.accent : Palette.border, lineWidth: 1))
        .background(GeometryReader { geometry in
            Color.clear
                .onAppear { frame = geometry.frame(in: .global) }
                .onChange(of: geometry.frame(in: .global)) { _, now in
                    frame = now
                    if focused { typing.field = now }
                }
        })
        .onChange(of: focused) { _, now in
            if now {
                typing.field = frame
            } else if typing.field == frame {
                typing.field = nil
            }
        }
    }

    private func run(_ text: String) {
        explore.searching = !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        search(text)
    }
}

/// Where to browse: every source, under where it comes from, with a box to
/// find one.
private struct SourceList: View {
    @Bindable var explore: Explore

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HavenField(placeholder: "Find a source", text: $explore.words)
                .accessibilityLabel("Find a source")
                .padding(.horizontal, 16)
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 2) {
                    ForEach(explore.rows) { row in
                        if row.header {
                            Text(row.label)
                                .font(Palette.font(11, .bold))
                                .foregroundStyle(Palette.accent)
                                .lineLimit(1)
                                .padding(.horizontal, 4)
                                .padding(.top, 12)
                                .padding(.bottom, 4)
                                .accessibilityAddTraits(.isHeader)
                        } else {
                            Button { explore.chooseSource(row.index) } label: {
                                Text(row.label)
                                    .font(Palette.font(15, row.index == explore.source ? .semibold : .regular))
                                    .foregroundStyle(Palette.text)
                                    .lineLimit(1)
                                    .padding(.horizontal, 12)
                                    .frame(maxWidth: .infinity, minHeight: 40, alignment: .leading)
                                    .background(RoundedRectangle(cornerRadius: 10)
                                        .fill(row.index == explore.source ? Palette.selected : Color.clear))
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(PressedStyle())
                            .accessibilityAddTraits(row.index == explore.source ? .isSelected : [])
                        }
                    }
                }
                .padding(.horizontal, 16)
                .padding(.bottom, 16)
            }
            .scrollDismissesKeyboard(.immediately)
        }
    }
}

/// The chosen source's titles, three across, with Films/Series and the
/// genre above them. Pull down to load it afresh.
private struct TitleGrid: View {
    let model: AppModel
    let explore: Explore

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if !explore.kinds.isEmpty || !explore.genres.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 10) {
                        ForEach(Array(explore.kinds.enumerated()), id: \.offset) { index, label in
                            Chip(label: label, selected: explore.kind == index) { explore.chooseKind(index) }
                        }
                        if !explore.genres.isEmpty { genreMenu }
                    }
                    .padding(.horizontal, 16)
                }
            }
            if !explore.note.isEmpty || (explore.loading && explore.items.isEmpty) {
                Text(explore.note.isEmpty ? "Loading…" : explore.note)
                    .font(Palette.font(14))
                    .foregroundStyle(Palette.textDim)
                    .lineLimit(2)
                    .padding(.horizontal, 16)
            }
            GeometryReader { geometry in
                let fit = PosterFit.grid(geometry.size.width)
                let card = fit.card
                ScrollView {
                    LazyVGrid(columns: Array(repeating: GridItem(.fixed(card), spacing: 10, alignment: .top), count: fit.count),
                              alignment: .leading, spacing: 14) {
                        ForEach(explore.items, id: \.self) { meta in
                            Button {
                                model.open(Opening(id: meta.id, kind: meta.kind, title: meta.name, poster: meta.poster))
                            } label: {
                                PosterCard(id: meta.id, title: meta.name, poster: meta.poster, rating: meta.rating,
                                           watched: model.library.filmWatched(kind: meta.kind, id: meta.id), width: card)
                            }
                            .buttonStyle(PressedStyle())
                        }
                    }
                    .padding(.horizontal, 16)
                    if explore.more && !explore.items.isEmpty {
                        Button { explore.load() } label: {
                            Text(explore.loading ? "Loading…" : "Load more")
                                .font(Palette.font(14, .semibold))
                                .foregroundStyle(Palette.text)
                                .padding(.horizontal, 20)
                                .frame(height: 40)
                                .background(Capsule().fill(Palette.surface))
                                .overlay(Capsule().strokeBorder(Palette.border, lineWidth: 1))
                        }
                        .buttonStyle(PressedStyle())
                        .disabled(explore.loading)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, 16)
                        .padding(.top, 8)
                    }
                    Color.clear.frame(height: 24)
                }
                .refreshable { await explore.refresh() }
                .tint(Palette.accent)
            }
        }
    }

    /// The first choice stands for every genre.
    private var genreMenu: some View {
        Menu {
            ForEach(Array(explore.genres.enumerated()), id: \.offset) { choice, name in
                Button {
                    explore.chooseGenre(choice)
                } label: {
                    if choice == explore.genre + 1 {
                        Label(name, systemImage: "checkmark")
                    } else {
                        Text(name)
                    }
                }
            }
        } label: {
            HStack(spacing: 6) {
                Text(explore.genres.indices.contains(explore.genre + 1) ? explore.genres[explore.genre + 1] : "All genres")
                    .font(Palette.font(14, .semibold))
                    .lineLimit(1)
                HavenIcon(name: "forward", size: 12, tint: Palette.textDim)
                    .rotationEffect(.degrees(90))
            }
            .foregroundStyle(Palette.text)
            .padding(.horizontal, 14)
            .frame(height: 34)
            .background(Capsule().fill(Palette.surface))
            .overlay(Capsule().strokeBorder(Palette.border, lineWidth: 1))
        }
        .accessibilityLabel("Genre")
    }
}

/// A round choice: gold when chosen.
struct Chip: View {
    let label: String
    var selected = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(label)
                .font(Palette.font(14, .semibold))
                .foregroundStyle(selected ? Palette.onAccent : Palette.text)
                .lineLimit(1)
                .padding(.horizontal, 14)
                .frame(height: 34)
                .background(Capsule().fill(selected ? Palette.accent : Palette.surface))
                .overlay(Capsule().strokeBorder(selected ? Color.clear : Palette.border, lineWidth: 1))
        }
        .buttonStyle(PressedStyle())
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}
