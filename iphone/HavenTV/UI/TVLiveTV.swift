import SwiftUI

/// Live TV on the television: TV or Radio, the categories down the left
/// (Favourites groups first, with a box to find one) and the chosen
/// category's channels on the right. Hold Select on a channel for its Favourites groups.
struct TVLiveTVPage: View {
    let model: AppModel
    @Bindable var live: LiveTVModel
    @State private var words = ""
    @State private var find = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 30) {
            HStack(spacing: 24) {
                TVChip(label: "TV", selected: !live.isRadio) { live.band = 0 }
                TVChip(label: "Radio", selected: live.isRadio) { live.band = 1 }
                Spacer(minLength: 0)
                if !live.board.channels.isEmpty {
                    TextField("Filter by name", text: $words)
                        .font(Palette.font(26))
                        .frame(width: 560)
                        .onSubmit { live.setWords(words) }
                        .onChange(of: words) { _, now in live.setWords(now) }
                    Text(live.countText).font(Palette.font(22)).foregroundStyle(Palette.textDim)
                }
            }
            .focusSection()
            if live.board.channels.isEmpty {
                TVNote(text: live.isRadio ? "Stations marked as radio in your Live TV sources show here."
                       : "Add an M3U playlist or an Xtream Codes account in Settings, under Live TV, to see the channels here.")
                if !live.isRadio {
                    TVChip(label: "Open Settings", selected: true) { model.select(.settings) }
                }
                Spacer(minLength: 0)
            } else {
                HStack(alignment: .top, spacing: 40) {
                    categories.frame(width: 520).focusSection()
                    channels.frame(maxWidth: .infinity).focusSection()
                }
            }
        }
        .padding(.horizontal, TV.side)
        .padding(.top, 40)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .onAppear {
            words = live.board.words
            find = live.board.categoryWords
        }
        .onChange(of: live.band) { _, _ in find = live.board.categoryWords }
        .sheet(isPresented: Binding(get: { live.choosing != nil }, set: { if !$0 { live.closeChooser() } })) {
            TVFavourites(live: live)
        }
    }

    private var categories: some View {
        ScrollView {
            LazyVStack(spacing: 12) {
                TextField("Find a category", text: $find)
                    .font(Palette.font(24))
                    .onSubmit { live.setCategoryWords(find) }
                    .onChange(of: find) { _, now in live.setCategoryWords(now) }
                if live.categories.isEmpty && !find.isEmpty {
                    TVNote(text: "No category has that in its name.")
                }
                ForEach(live.categories, id: \.key) { row in
                    TVRow(selected: row.key == live.board.category) {
                        live.choose(row.key)
                    } content: {
                        if row.favourite { Text("★").font(Palette.font(24)).foregroundStyle(Palette.accent) }
                        Text(row.label).font(Palette.font(24, .semibold)).foregroundStyle(Palette.text).lineLimit(1)
                        Spacer(minLength: 0)
                        Text("\(row.count)").font(Palette.font(20)).foregroundStyle(Palette.textDim)
                    }
                    .contextMenu {
                        if row.favourite {
                            Button("Delete the group \(row.label)", role: .destructive) { live.deleteGroup(row.label) }
                        }
                    }
                }
            }
            .padding(8)
        }
        .scrollClipDisabled()
    }

    private var channels: some View {
        let rows = live.channels
        return ScrollView {
            LazyVStack(spacing: 12) {
                if rows.isEmpty {
                    TVNote(text: "Nothing here. Hold Select on a channel to put it in a Favourites group.")
                }
                ForEach(rows, id: \.index) { channel in
                    TVRow {
                        live.play(channel.index)
                    } content: {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(channel.name).font(Palette.font(26, .semibold)).foregroundStyle(Palette.text).lineLimit(1)
                            if !channel.group.isEmpty {
                                Text(channel.group).font(Palette.font(20)).foregroundStyle(Palette.textDim).lineLimit(1)
                            }
                        }
                        Spacer(minLength: 0)
                        if channel.starred { Text("★").font(Palette.font(26)).foregroundStyle(Palette.accent) }
                        HavenIcon(name: "play", size: 26, tint: Palette.accent)
                    }
                    .contextMenu {
                        Button("Favourites…") { live.chooseFavourites(channel.index) }
                    }
                }
            }
            .padding(8)
        }
        .scrollClipDisabled()
    }
}

/// Choosing the Favourites groups of one channel.
private struct TVFavourites: View {
    let live: LiveTVModel
    @State private var name = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            Text("Favourites").font(Palette.heading(44)).foregroundStyle(Palette.text)
            Text(live.choosing?.name ?? "").font(Palette.font(26)).foregroundStyle(Palette.textDim)
            if live.favourites.isEmpty {
                TVNote(text: "You have no Favourites groups yet. Name one below and this channel goes into it.")
            }
            ScrollView {
                VStack(spacing: 12) {
                    ForEach(live.favourites, id: \.name) { group in
                        TVRow(selected: live.has(group)) {
                            live.toggle(group.name)
                        } content: {
                            Text(group.name).font(Palette.font(26)).foregroundStyle(Palette.text)
                            Spacer()
                            if live.has(group) { HavenIcon(name: "check", size: 28, tint: Palette.accent) }
                        }
                    }
                }
                .padding(8)
            }
            HStack(spacing: 20) {
                TextField("New group, for example Sport", text: $name)
                    .font(Palette.font(26))
                    .onSubmit { add() }
                TVChip(label: "New group") { add() }
                TVChip(label: "Done", selected: true) { live.closeChooser() }
            }
        }
        .padding(80)
        .frame(width: 1200)
    }

    private func add() {
        guard !name.trimmingCharacters(in: .whitespaces).isEmpty else { return }
        live.newGroup(name)
        name = ""
    }
}
