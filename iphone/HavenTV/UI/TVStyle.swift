import SwiftUI

/// Sizes for a television seen from the sofa: the same palette and fonts as
/// the iPhone app, larger, and every control shows clearly when the remote
/// is on it (lifted, with a gold edge).
enum TV {
    /// Space kept clear at the screen's edges (tvOS's safe area is 60/80).
    static let side: CGFloat = 80
    static let poster: CGFloat = 230
    static let wide: CGFloat = 440
}

/// Lifts what the remote is on and gives it a gold edge.
struct TVFocusStyle: ButtonStyle {
    var shape: AnyShape = AnyShape(RoundedRectangle(cornerRadius: 16))
    var lift: CGFloat = 1.06

    func makeBody(configuration: Configuration) -> some View {
        Lifted(configuration: configuration, shape: shape, lift: lift)
    }

    private struct Lifted: View {
        let configuration: Configuration
        let shape: AnyShape
        let lift: CGFloat
        @Environment(\.isFocused) private var focused

        var body: some View {
            configuration.label
                .overlay(shape.stroke(Palette.accent, lineWidth: focused ? 4 : 0))
                .scaleEffect(focused ? lift : 1)
                .shadow(color: .black.opacity(focused ? 0.45 : 0), radius: 18, y: 10)
                .opacity(configuration.isPressed ? 0.8 : 1)
                .animation(.easeOut(duration: 0.15), value: focused)
        }
    }
}

/// A round choice in Haven's colours: gold when chosen; white when the
/// remote is on it.
struct TVChip: View {
    let label: String
    var selected = false
    var icon: String?
    let action: () -> Void

    var body: some View {
        Button(action: action) { ChipFace(label: label, selected: selected, icon: icon) }
            .buttonStyle(TVPlainStyle())
            .accessibilityAddTraits(selected ? .isSelected : [])
    }

    private struct ChipFace: View {
        let label: String
        let selected: Bool
        let icon: String?
        @Environment(\.isFocused) private var focused

        var body: some View {
            HStack(spacing: 10) {
                if let icon { HavenIcon(name: icon, size: 26, tint: ink, stroke: 2.4) }
                Text(label).font(Palette.font(26, .semibold)).lineLimit(1)
            }
            .foregroundStyle(ink)
            .padding(.horizontal, 26)
            .frame(height: 62)
            .background(Capsule().fill(focused ? Color.white : selected ? Palette.accent : Palette.surface))
            .overlay(Capsule().strokeBorder(focused || selected ? Color.clear : Palette.border, lineWidth: 1))
            .scaleEffect(focused ? 1.06 : 1)
            .shadow(color: .black.opacity(focused ? 0.4 : 0), radius: 12, y: 6)
            .animation(.easeOut(duration: 0.15), value: focused)
        }

        private var ink: Color { focused ? Palette.onAccent : selected ? Palette.onAccent : Palette.text }
    }
}

/// A button whose label draws its own focus look (reads `isFocused`).
struct TVPlainStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label.opacity(configuration.isPressed ? 0.8 : 1)
    }
}

/// A full-width row in a list (Settings, Live TV, episodes, sources).
struct TVRow<Content: View>: View {
    var selected = false
    let action: () -> Void
    @ViewBuilder var content: () -> Content

    var body: some View {
        Button(action: action) { Face(selected: selected, content: content) }
            .buttonStyle(TVPlainStyle())
            .accessibilityAddTraits(selected ? .isSelected : [])
    }

    private struct Face: View {
        let selected: Bool
        let content: () -> Content
        @Environment(\.isFocused) private var focused

        var body: some View {
            HStack(spacing: 18) { content() }
                .padding(.horizontal, 24)
                .padding(.vertical, 14)
                .frame(maxWidth: .infinity, minHeight: 70, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: 14)
                    .fill(focused ? Palette.hover : selected ? Palette.selected : Palette.surface.opacity(0.85)))
                .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(focused ? Palette.accent : Palette.border, lineWidth: focused ? 3 : 1))
                .scaleEffect(focused ? 1.02 : 1)
                .animation(.easeOut(duration: 0.15), value: focused)
        }
    }
}

/// A heading over a row or a part of a page.
struct TVHeading: View {
    let text: String
    var detail = ""

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 14) {
            Text(text).font(Palette.heading(34)).foregroundStyle(Palette.text)
            if !detail.isEmpty {
                Text(detail).font(Palette.font(22)).foregroundStyle(Palette.textDim)
            }
        }
    }
}

/// Small dim text.
struct TVNote: View {
    let text: String
    var error = false

    var body: some View {
        Text(text)
            .font(Palette.font(24))
            .foregroundStyle(error ? Palette.alert : Palette.textDim)
            .fixedSize(horizontal: false, vertical: true)
    }
}

extension Color {
    /// A colour written as 0xRRGGBB.
    init(hex: UInt32) {
        self.init(red: Double((hex >> 16) & 0xff) / 255, green: Double((hex >> 8) & 0xff) / 255, blue: Double(hex & 0xff) / 255)
    }
}
