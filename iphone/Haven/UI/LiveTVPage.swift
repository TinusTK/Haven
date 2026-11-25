import SwiftUI

/// Live TV: television or radio, the chosen category's channels, and (on
/// asking) the categories with the Favourites groups first.
struct LiveTVPage: View {
    let model: AppModel
    @Bindable var live: LiveTVModel

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 12) {
                PageTitle(text: "Live TV")
                Spacer(minLength: 0)
                Segments(labels: ["TV", "Radio"], current: $live.band)
                    .fixedSize()
            }
            .padding(.horizontal, 16)
            if live.board.channels.isEmpty {
                VStack(alignment: .leading, spacing: 12) {
                    Text(live.isRadio ? "Stations marked as radio in your Live TV sources show here."
                         : "Add an M3U playlist or an Xtream Codes account in Settings, under Connections › Live TV, to see the channels here.")
                        .font(Palette.font(14))
                        .foregroundStyle(Palette.textDim)
                    if !live.isRadio {
                        PrimaryButton(text: "Open Settings") { model.select(.settings) }
                    }
                }
                .padding(.horizontal, 16)
                Spacer(minLength: 0)
            } else {
                Chip(label: live.picking ? "‹  Back to the channels" : "\(live.categoryName)  ›", selected: live.picking) {
                    live.picking.toggle()
                }
                .accessibilityLabel(live.picking ? "Back to the channels" : "Categories, now \(live.categoryName)")
                .padding(.horizontal, 16)
                if live.picking {
                    CategoryList(live: live)
                } else {
                    ChannelList(live: live)
                }
            }
        }
        .padding(.top, 12)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .onChange(of: live.band) { _, _ in live.picking = false }
        .sheet(isPresented: Binding(get: { live.choosing != nil }, set: { if !$0 { live.closeChooser() } })) {
            FavouritesSheet(live: live)
                .presentationDetents([.medium, .large])
        }
    }
}

/// The categories: Favourites groups (with ★), every channel, the
/// playlists' own groups; a box to find one and one to make a group.
private struct CategoryList: View {
    let live: LiveTVModel
    @State private var find = ""
    @State private var newGroup = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HavenField(placeholder: "Find a category", text: $find, edited: { live.setCategoryWords($0) })
                .onAppear { find = live.board.categoryWords }
            HStack(spacing: 6) {
                HavenField(placeholder: "New Favourites group", text: $newGroup, submit: { make($0) })
                Chip(label: "Add") { make(newGroup) }
                    .disabled(newGroup.trimmingCharacters(in: .whitespaces).isEmpty)
                    .accessibilityLabel("Add the Favourites group")
            }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 2) {
                    ForEach(live.categories, id: \.key) { row in
                        HStack(spacing: 8) {
                            Button { live.choose(row.key) } label: {
                                HStack(spacing: 8) {
                                    if row.favourite { Text("★").font(Palette.font(14)).foregroundStyle(Palette.accent) }
                                    Text(row.label)
                                        .font(Palette.font(15, row.key == live.board.category ? .semibold : .regular))
                                        .foregroundStyle(Palette.text)
                                        .lineLimit(1)
                                    Spacer(minLength: 0)
                                    Text("\(row.count)").font(Palette.font(12)).foregroundStyle(Palette.textDim)
                                }
                                .padding(.horizontal, 12)
                                .frame(minHeight: 42)
                                .background(RoundedRectangle(cornerRadius: 10)
                                    .fill(row.key == live.board.category ? Palette.selected : Color.clear))
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(PressedStyle())
                            .accessibilityAddTraits(row.key == live.board.category ? .isSelected : [])
                            if row.favourite {
                                Button { live.deleteGroup(row.label) } label: {
                                    HavenIcon(name: "trash", size: 16, tint: Palette.textDim)
                                        .frame(width: 36, height: 36)
                                }
                                .buttonStyle(PressedStyle())
                                .accessibilityLabel("Delete the Favourites group \(row.label)")
                            }
                        }
                    }
                }
                .padding(.bottom, 16)
            }
            .scrollDismissesKeyboard(.immediately)
        }
        .padding(.horizontal, 16)
    }

    private func make(_ name: String) {
        live.newGroup(name)
        newGroup = ""
    }
}

/// The chosen category's channels, with a filter.
private struct ChannelList: View {
    let live: LiveTVModel
    @State private var words = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 12) {
                HavenField(placeholder: "Filter by name", text: $words, edited: { live.setWords($0) })
                    .onAppear { words = live.board.words }
                Text(live.countText).font(Palette.font(12)).foregroundStyle(Palette.textDim).fixedSize()
            }
            .padding(.horizontal, 16)
            let rows = live.channels
            if rows.isEmpty {
                Text("Nothing here. Use the ☆ on a channel to put it in a Favourites group.")
                    .font(Palette.font(14))
                    .foregroundStyle(Palette.textDim)
                    .padding(.horizontal, 16)
            }
            ScrollView {
                LazyVStack(spacing: 6) {
                    ForEach(rows, id: \.index) { channel in
                        ChannelItem(channel: channel) {
                            live.play(channel.index)
                        } star: {
                            live.chooseFavourites(channel.index)
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

/// One channel: its name and group, Play, and the ☆ for Favourites.
private struct ChannelItem: View {
    let channel: LiveTV.ChannelRow
    let play: () -> Void
    let star: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Button(action: play) {
                HStack(spacing: 12) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(channel.name).font(Palette.font(14, .semibold)).foregroundStyle(Palette.text).lineLimit(1)
                        if !channel.group.isEmpty {
                            Text(channel.group).font(Palette.font(12)).foregroundStyle(Palette.textDim).lineLimit(1)
                        }
                    }
                    Spacer(minLength: 0)
                    Text("▶\u{FE0E}").font(Palette.font(13)).foregroundStyle(Palette.accent)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Play \(channel.name)")
            Button(action: star) {
                Text(channel.starred ? "★" : "☆")
                    .font(Palette.font(20))
                    .foregroundStyle(channel.starred ? Palette.accent : Palette.textDim)
                    .frame(width: 36, height: 36)
            }
            .buttonStyle(PressedStyle())
            .accessibilityLabel(channel.starred ? "Change the Favourites groups of \(channel.name)" : "Add to Favourites: \(channel.name)")
        }
        .padding(.horizontal, 14)
        .frame(minHeight: 52)
        .background(RoundedRectangle(cornerRadius: 10).fill(Palette.surface))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Palette.border, lineWidth: 1))
    }
}

/// Choosing the Favourites groups of one channel.
private struct FavouritesSheet: View {
    let live: LiveTVModel
    @State private var name = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Favourites").font(Palette.heading(20)).foregroundStyle(Palette.text)
            Text(live.choosing?.name ?? "").font(Palette.font(14)).foregroundStyle(Palette.textDim).lineLimit(1)
            if live.favourites.isEmpty {
                Text("You have no Favourites groups yet. Name one below and this channel goes into it.")
                    .font(Palette.font(14))
                    .foregroundStyle(Palette.textDim)
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(live.favourites, id: \.name) { group in
                        Button { live.toggle(group.name) } label: {
                            HStack {
                                Text(group.name).font(Palette.font(15)).foregroundStyle(Palette.text)
                                Spacer()
                                if live.has(group) { HavenIcon(name: "check", size: 18, tint: Palette.accent) }
                            }
                            .padding(.horizontal, 12)
                            .frame(minHeight: 44)
                            .background(RoundedRectangle(cornerRadius: 10).fill(live.has(group) ? Palette.selected : Palette.surface))
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(PressedStyle())
                        .accessibilityAddTraits(live.has(group) ? .isSelected : [])
                    }
                }
            }
            HStack(spacing: 8) {
                HavenField(placeholder: "New group, for example Sport", text: $name, submit: { add($0) })
                Chip(label: "New group") { add(name) }
                    .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            PrimaryButton(text: "Done") { live.closeChooser() }
        }
        .padding(20)
        .background(Palette.background)
    }

    private func add(_ text: String) {
        live.newGroup(text)
        name = ""
    }
}
