import SwiftUI

/// The page showing. Each page is filled in by its own section (Home S10,
/// Explore S11, Library S14, Live TV S15, Calendar S16, Settings S17);
/// until then it shows its title and what it will hold.
struct PageView: View {
    let model: AppModel

    var body: some View {
        switch model.page {
        case .home: HomePage(model: model)
        case .explore: ExplorePage(model: model)
        case .library: Placeholder(title: "Library", note: "Local, Downloads, Watchlist and History come here.")
        case .liveTV: Placeholder(title: "Live TV", note: "Your channels and radio stations come here.")
        case .calendar: Placeholder(title: "Calendar", note: "What comes out from your Watchlist comes here.")
        case .settings: SettingsPlaceholder(model: model)
        }
    }
}

private struct Placeholder: View {
    let title: String
    let note: String

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 10) {
                PageTitle(text: title)
                Text(note)
                    .font(Palette.font(14))
                    .foregroundStyle(Palette.textDim)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16)
            .padding(.top, 12)
        }
        .scrollDismissesKeyboard(.immediately)
    }
}

private struct SettingsPlaceholder: View {
    let model: AppModel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                PageTitle(text: "Settings")
                VStack(alignment: .leading, spacing: 10) {
                    Text("APPEARANCE")
                        .font(Palette.font(12, .bold))
                        .foregroundStyle(Palette.textDim)
                    Toggle(isOn: Binding(get: { model.dark }, set: { model.setDark($0) })) {
                        Text("Dark theme")
                            .font(Palette.font(15, .semibold))
                            .foregroundStyle(Palette.text)
                    }
                    .tint(Palette.accent)
                }
                .padding(14)
                .background(RoundedRectangle(cornerRadius: 14).fill(Palette.surface))
                .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(Palette.border, lineWidth: 1))
                Text("The other settings come here.")
                    .font(Palette.font(14))
                    .foregroundStyle(Palette.textDim)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16)
            .padding(.top, 12)
        }
    }
}

/// Stands in for a title's page until S12: its name, and the way back.
struct TitlePlaceholder: View {
    let opening: Opening
    let close: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Button(action: close) {
                HStack(spacing: 4) {
                    HavenIcon(name: "back", size: 18, tint: Palette.text)
                    Text("Back").font(Palette.font(14, .semibold)).foregroundStyle(Palette.text)
                }
                .padding(.horizontal, 12)
                .frame(height: 36)
                .background(Capsule().fill(Palette.surface))
            }
            .buttonStyle(.plain)
            PageTitle(text: opening.title)
            Text("The title's page comes next.")
                .font(Palette.font(14))
                .foregroundStyle(Palette.textDim)
            Spacer()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(16)
        .background(Palette.background)
    }
}
