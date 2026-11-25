import SwiftUI

/// Haven's colours (dark and light, from the Rust build's Theme) and type:
/// Figtree for text, Pixelify Sans for headings. The fonts' digits were
/// taken out (as in the Rust build), so numbers show in the system's font.
enum Palette {
    static let background = Color("Background")
    static let surface = Color("Surface")
    static let border = Color("Border")
    static let hover = Color("Hover")
    static let selected = Color("Selected")
    static let text = Color("Text")
    static let textDim = Color("TextDim")
    static let accent = Color("Accent")
    /// Stands out on purpose: the "Report an issue" pill.
    static let alert = Color("Alert")
    /// Text on the gold accent (dark in both themes, as in the Rust build).
    static let onAccent = Color(red: 0x17 / 255, green: 0x1a / 255, blue: 0x22 / 255)

    /// Body text: Figtree.
    static func font(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
        .custom("Figtree", size: size).weight(weight)
    }

    /// Headings: Pixelify Sans.
    static func heading(_ size: CGFloat) -> Font {
        .custom("Pixelify Sans", size: size).weight(.bold)
    }
}

/// A page's title ("Home", "Library"…).
struct PageTitle: View {
    let text: String

    var body: some View {
        Text(text)
            .font(Palette.heading(26))
            .foregroundStyle(Palette.text)
    }
}

/// The gold button.
struct PrimaryButton: View {
    let text: String
    var enabled = true
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(text)
                .font(Palette.font(13, .bold))
                .foregroundStyle(enabled ? Palette.onAccent : Palette.textDim)
                .padding(.horizontal, 18)
                .frame(minHeight: 34)
                .background(Capsule().fill(enabled ? Palette.accent : Palette.selected))
        }
        .buttonStyle(PressedStyle())
        .disabled(!enabled)
        .accessibilityLabel(text)
    }
}

/// A button that darkens a little while pressed (Haven's buttons have no
/// hover on a touch screen).
struct PressedStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label.opacity(configuration.isPressed ? 0.75 : 1)
    }
}
