import Observation
import SwiftUI
import UIKit

/// The text field being typed in, so a tap anywhere else closes the keyboard
/// (and does nothing else), as in the Rust build: the window lays a clear
/// layer over everything but the field.
@MainActor @Observable
final class Typing {
    /// Where the field with the keyboard is, in the window; nil when none.
    var field: CGRect?

    func close() {
        UIApplication.shared.sendAction(#selector(UIResponder.resignFirstResponder), to: nil, from: nil, for: nil)
        field = nil
    }
}

/// A text field with Haven's look, and a Paste button beside it (a phone has
/// no keyboard shortcut for it). Apple's Paste button pastes without asking
/// for permission each time.
struct HavenField: View {
    let placeholder: String
    @Binding var text: String
    var secure = false
    var submit: (String) -> Void = { _ in }
    var edited: (String) -> Void = { _ in }

    @Environment(Typing.self) private var typing
    @FocusState private var focused: Bool
    @State private var frame: CGRect = .zero

    var body: some View {
        HStack(spacing: 6) {
            Group {
                if secure {
                    SecureField("", text: $text, prompt: prompt)
                } else {
                    TextField("", text: $text, prompt: prompt)
                }
            }
            .font(Palette.font(14))
            .foregroundStyle(Palette.text)
            .tint(Palette.accent)
            .autocorrectionDisabled()
            .textInputAutocapitalization(.never)
            .focused($focused)
            .onSubmit { submit(text) }
            .onChange(of: text) { _, now in edited(now) }
            .padding(.horizontal, 12)
            .frame(height: 38)
            .background(RoundedRectangle(cornerRadius: 10).fill(Palette.surface))
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(focused ? Palette.accent : Palette.border, lineWidth: 1))
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
            PasteButton(payloadType: String.self) { pasted in
                guard let first = pasted.first else { return }
                Task { @MainActor in
                    text = first
                    edited(first)
                }
            }
            .labelStyle(.iconOnly)
            .buttonBorderShape(.capsule)
            .tint(Palette.selected)
            .foregroundStyle(Palette.text)
            .accessibilityLabel("Paste")
        }
    }

    private var prompt: Text {
        Text(placeholder).foregroundStyle(Palette.textDim)
    }
}

/// Everything but `hole`: taps there close the keyboard.
struct OutsideShape: Shape {
    let hole: CGRect

    func path(in rect: CGRect) -> Path {
        var path = Path(rect)
        path.addRect(hole)
        return path
    }
}
