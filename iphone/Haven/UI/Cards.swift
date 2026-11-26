import SwiftUI
import UIKit

/// A picture from an address, loaded (and kept) by `Pictures`; `empty`
/// shows until it arrives, or if it never does.
struct RemotePicture<Empty: View>: View {
    let url: String?
    let size: PictureSize
    var pause: Pause?
    var loaded: () -> Void = {}
    @ViewBuilder var empty: () -> Empty
    @State private var image: UIImage?
    @State private var shownURL: String?

    var body: some View {
        Group {
            if let image, shownURL == url {
                Image(uiImage: image).resizable().scaledToFill()
            } else {
                empty()
            }
        }
        .task(id: url) {
            guard let url else {
                image = nil
                return
            }
            if let found = await Pictures.shared.load(url, size, pause: pause) {
                image = found
                shownURL = url
                loaded()
            }
        }
    }
}

/// A poster on a shelf: the picture (or the title when there is none), its
/// score in a dark pill, a gold tick when watched, a bar for how far it was
/// watched, and the title under it.
struct PosterCard: View {
    let id: String
    let title: String
    let poster: String?
    var rating: String?
    var watched = false
    var progress: Double = 0
    let width: CGFloat
    var pause: Pause?

    static func height(_ width: CGFloat) -> CGFloat { width * 1.5 + 30 }

    var body: some View {
        VStack(spacing: 7) {
            ZStack {
                RoundedRectangle(cornerRadius: 10).fill(Palette.surface)
                RemotePicture(url: poster, size: .poster, pause: pause) {
                    Text(title)
                        .font(Palette.font(13))
                        .foregroundStyle(Palette.textDim)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal, 10)
                }
                .frame(width: width, height: width * 1.5)
                .clipped()
                VStack {
                    HStack(alignment: .top) {
                        Spacer()
                        if let rating, !rating.isEmpty {
                            Text("★ \(rating)")
                                .font(Palette.font(12, .bold))
                                .foregroundStyle(.white)
                                .padding(.horizontal, 8)
                                .frame(height: 24)
                                .background(RoundedRectangle(cornerRadius: 7).fill(Color(white: 0.04, opacity: 0.78)))
                        } else if watched {
                            WatchedTick(size: 26)
                        }
                    }
                    Spacer()
                    if watched, let rating, !rating.isEmpty {
                        HStack { Spacer(); WatchedTick(size: 26) }
                    }
                    if progress > 0 && !watched {
                        ProgressBar(share: progress, track: Color.black.opacity(0.63))
                            .padding(.horizontal, 8)
                            .padding(.bottom, 8)
                    }
                }
                .padding(7)
            }
            .frame(width: width, height: width * 1.5)
            .clipShape(RoundedRectangle(cornerRadius: 10))
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Palette.border, lineWidth: 1))
            Text(title)
                .font(Palette.font(14, .semibold))
                .foregroundStyle(Palette.text)
                .lineLimit(1)
                .frame(width: width)
        }
        .frame(width: width, height: Self.height(width), alignment: .top)
        .contentShape(Rectangle())
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(rating.map { "\(title), rated \($0)" } ?? title)
        .accessibilityAddTraits(.isButton)
    }
}

/// Watched: a gold tick in a round pill.
struct WatchedTick: View {
    var size: CGFloat = 22

    var body: some View {
        Circle()
            .fill(Color(red: 0x11 / 255, green: 0x13 / 255, blue: 0x1a / 255).opacity(0.8))
            .overlay(Circle().strokeBorder(Palette.accent, lineWidth: 1.5))
            .overlay(HavenIcon(name: "check", size: size * 0.64, tint: Palette.accent, stroke: 2.5))
            .frame(width: size, height: size)
            .accessibilityLabel("Watched")
    }
}

/// A thin gold bar for how far something was watched.
struct ProgressBar: View {
    let share: Double
    var track = Color.white.opacity(0.25)

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .leading) {
                Capsule().fill(track)
                Capsule().fill(Palette.accent).frame(width: geometry.size.width * min(max(share, 0), 1))
            }
        }
        .frame(height: 4)
    }
}

/// A wide card (Upcoming, Continue Watching): a 16:9 picture with a badge
/// and pills, and two lines under it.
struct WideCard<Corner: View>: View {
    let title: String
    let line: String
    let picture: String?
    let badge: String
    let width: CGFloat
    var progress: Double?
    var pause: Pause?
    @ViewBuilder var corner: () -> Corner

    static func height(_ width: CGFloat) -> CGFloat { width * 0.5625 + 64 }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ZStack(alignment: .bottomLeading) {
                RoundedRectangle(cornerRadius: 16).fill(Palette.surface)
                RemotePicture(url: picture, size: .wide, pause: pause) { Color.clear }
                    .frame(width: width, height: width * 0.5625)
                    .clipped()
                LinearGradient(colors: [.clear, .black.opacity(0.62)], startPoint: UnitPoint(x: 0.5, y: 0.5), endPoint: .bottom)
                if !badge.isEmpty {
                    Text(badge)
                        .font(Palette.font(12, .heavy))
                        .foregroundStyle(.white)
                        .padding(.leading, 16)
                        .padding(.bottom, progress == nil ? 14 : 18)
                }
                corner()
                if let progress {
                    ProgressBar(share: progress)
                        .padding(.horizontal, 12)
                        .padding(.bottom, 8)
                }
            }
            .frame(width: width, height: width * 0.5625)
            .clipShape(RoundedRectangle(cornerRadius: 16))
            .overlay(RoundedRectangle(cornerRadius: 16).strokeBorder(Palette.border, lineWidth: 1))
            Text(title)
                .font(Palette.font(15, .semibold))
                .foregroundStyle(Palette.text)
                .lineLimit(1)
                .padding(.top, 10)
            Text(line)
                .font(Palette.font(13))
                .foregroundStyle(Palette.textDim)
                .lineLimit(1)
                .padding(.top, 4)
        }
        .frame(width: width, height: Self.height(width), alignment: .top)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isButton)
    }
}

/// A row's heading with an arrow that opens the page behind it.
struct RowHeading: View {
    let title: String
    var action: (() -> Void)?

    var body: some View {
        HStack(spacing: 6) {
            Text(title)
                .font(Palette.heading(20))
                .foregroundStyle(Palette.text)
            if let action {
                Button(action: action) {
                    HavenIcon(name: "forward", size: 18, tint: Palette.textDim)
                        .frame(width: 26, height: 26)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Open \(title)")
            }
        }
    }
}

/// Poster sizes that suit the width: three across a phone, more across an
/// iPad (posters about 160 points wide there).
enum PosterFit {
    /// Wider than this, the page is laid out for an iPad.
    static let wide: CGFloat = 700

    /// Columns and card width for a grid `width` wide (its padding of 16 a
    /// side and gaps of 10 already taken off).
    static func grid(_ width: CGFloat) -> (count: Int, card: CGFloat) {
        let inside = max(width - 32, 1)
        let count = width < wide ? 3 : max(3, Int((inside + 10) / 170))
        return (count, max((inside - 10 * CGFloat(count - 1)) / CGFloat(count), 1))
    }

    /// A poster in a row `width` wide: 3.15 across a phone.
    static func row(_ width: CGFloat) -> CGFloat {
        width < wide ? max((width - 24) / 3.15, 1) : 160
    }
}
