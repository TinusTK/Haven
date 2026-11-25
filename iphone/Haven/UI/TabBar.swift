import SwiftUI

/// Haven's pages along the bottom: six buttons on a floating pill (icon
/// above a small bold label), as the Rust build's phone layout has it.
struct TabBar: View {
    let page: Page
    /// A title's page is open: no tab is highlighted.
    let detailOpen: Bool
    let select: (Page) -> Void

    /// The strip's height above the home bar.
    static let height: CGFloat = 78

    var body: some View {
        GeometryReader { geometry in
            let button = max((geometry.size.width - 24 - 10) / 6, 0)
            HStack(spacing: 0) {
                ForEach(Page.allCases) { tab in
                    TabButton(page: tab, selected: !detailOpen && page == tab) { select(tab) }
                        .frame(width: button)
                }
            }
            .padding(5)
            .background(
                RoundedRectangle(cornerRadius: 33)
                    .fill(Palette.surface)
                    .overlay(RoundedRectangle(cornerRadius: 33).strokeBorder(Palette.border, lineWidth: 1))
                    .shadow(color: .black.opacity(scheme == .dark ? 0.5 : 0.15), radius: 11, y: 6))
            .frame(height: 66)
            .padding(.horizontal, 12)
            .padding(.top, 6)
        }
        .frame(height: Self.height)
    }

    /// The pill's shadow is deeper in the dark theme.
    @Environment(\.colorScheme) private var scheme
}

private struct TabButton: View {
    let page: Page
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: 3) {
                HavenIcon(name: page.icon, size: 22, tint: selected ? Palette.text : Palette.textDim)
                Text(page.label)
                    .font(Palette.font(11, .bold))
                    .foregroundStyle(selected ? Palette.text : Palette.textDim)
                    .lineLimit(1)
                    .minimumScaleFactor(0.8)
            }
            .frame(maxWidth: .infinity)
            .frame(height: 56)
            .background(Capsule().fill(selected ? Palette.selected : .clear))
            .contentShape(Capsule())
            .animation(.easeInOut(duration: 0.15), value: selected)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(page.label)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}
