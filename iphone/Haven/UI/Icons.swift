import SwiftUI

/// Haven's own icons: the Rust build's line drawings (24×24 paths, drawn
/// with a 2-point line; "play" is filled), so they look the same.
struct HavenIcon: View {
    let name: String
    var size: CGFloat = 24
    var tint: Color = Palette.text
    var stroke: CGFloat = 2

    var body: some View {
        let shape = IconShape(name: name)
        Group {
            if name == "play" {
                shape.fill(tint)
            } else {
                shape.stroke(tint, style: StrokeStyle(lineWidth: stroke))
            }
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }
}

/// An icon's path, scaled to the space given.
struct IconShape: Shape {
    let name: String

    func path(in rect: CGRect) -> Path {
        let scale = min(rect.width, rect.height) / 24
        return IconPaths.path(name).applying(CGAffineTransform(scaleX: scale, y: scale).translatedBy(x: rect.minX / scale, y: rect.minY / scale))
    }
}

enum IconPaths {
    /// The drawings, as SVG path text in a 24×24 box.
    static let drawings: [String: String] = [
        "home": "M3 10.5 L12 3 L21 10.5 M5.5 9 V20 a1 1 0 0 0 1 1 H10 V15 H14 V21 H17.5 a1 1 0 0 0 1 -1 V9",
        "explore": "M12 2.5 a9.5 9.5 0 1 0 0 19 a9.5 9.5 0 1 0 0 -19 Z M15.8 8.2 L13.6 13.6 L8.2 15.8 L10.4 10.4 Z",
        "library": "M4 4 H8 V20 H4 Z M10 4 H14 V20 H10 Z M15.6 5.2 L19.4 4.2 L22 19 L18.2 20 Z",
        "livetv": "M3 7 H21 a1 1 0 0 1 1 1 V18 a1 1 0 0 1 -1 1 H3 a1 1 0 0 1 -1 -1 V8 a1 1 0 0 1 1 -1 Z M8 3 L12 7 L16 3",
        "calendar": "M4 5 H20 a1 1 0 0 1 1 1 V20 a1 1 0 0 1 -1 1 H4 a1 1 0 0 1 -1 -1 V6 a1 1 0 0 1 1 -1 Z M3 10 H21 M8 3 V7 M16 3 V7",
        "search": "M10.5 3.5 a7 7 0 1 0 0 14 a7 7 0 1 0 0 -14 Z M15.5 15.5 L21 21",
        "settings": "M18.88 9.87 L21.66 10.34 L21.66 13.66 L18.88 14.13 L18.37 15.36 L20.00 17.66 L17.66 20.00 L15.36 18.37 L14.13 18.88 L13.66 21.66 L10.34 21.66 L9.87 18.88 L8.64 18.37 L6.34 20.00 L4.00 17.66 L5.63 15.36 L5.12 14.13 L2.34 13.66 L2.34 10.34 L5.12 9.87 L5.63 8.64 L4.00 6.34 L6.34 4.00 L8.64 5.63 L9.87 5.12 L10.34 2.34 L13.66 2.34 L14.13 5.12 L15.36 5.63 L17.66 4.00 L20.00 6.34 L18.37 8.64 Z M12 9 a3 3 0 1 0 0 6 a3 3 0 1 0 0 -6 Z",
        "download": "M12 3 V15 M7 10 L12 15 L17 10 M4 20 H20",
        "trash": "M4 7 H20 M9 7 V4 H15 V7 M6 7 L7 21 H17 L18 7",
        "play": "M7 4 L20 12 L7 20 Z",
        "pause": "M8 5 V19 M16 5 V19",
        "folder": "M3 6 a1 1 0 0 1 1 -1 H9 L11 7 H20 a1 1 0 0 1 1 1 V18 a1 1 0 0 1 -1 1 H4 a1 1 0 0 1 -1 -1 Z",
        "check": "M5 12.5 L10 17.5 L19 7",
        "close": "M6 6 L18 18 M18 6 L6 18",
        "back": "M15 5 L8 12 L15 19",
        "forward": "M9 5 L16 12 L9 19",
        "radio": "M4 9 H20 a1 1 0 0 1 1 1 V19 a1 1 0 0 1 -1 1 H4 a1 1 0 0 1 -1 -1 V10 a1 1 0 0 1 1 -1 Z M6 9 L17 4 M15.5 12.5 a2.5 2.5 0 1 0 0 5 a2.5 2.5 0 1 0 0 -5 Z M6 13 H11 M6 16 H11",
        "account": "M12 4 a4 4 0 1 0 0 8 a4 4 0 1 0 0 -8 Z M4 21 a8 8 0 0 1 16 0",
        "server": "M4 4 H20 V10 H4 Z M4 14 H20 V20 H4 Z M7 7 H8 M7 17 H8",
        "addon": "M5 8 H9 a2 2 0 1 1 4 0 H17 V12 a2 2 0 1 1 0 4 V20 H5 Z",
        "info": "M12 3 a9 9 0 1 0 0 18 a9 9 0 1 0 0 -18 Z M12 11 V16 M12 7.5 V8",
        "general": "M4 6 H20 M4 12 H20 M4 18 H20 M9 4 V8 M15 10 V14 M7 16 V20",
        "storage": "M4 5 H20 V19 H4 Z M4 14 H20 M7 16.5 H8",
        "paste": "M8 5 H6 a1 1 0 0 0 -1 1 V20 a1 1 0 0 0 1 1 H18 a1 1 0 0 0 1 -1 V6 a1 1 0 0 0 -1 -1 H16 M9 3 H15 V7 H9 Z M9 12 H15 M9 16 H13",
        "sync": "M20 11 a8 8 0 0 0 -14.5 -4.5 M4 4 V8 H8 M4 13 a8 8 0 0 0 14.5 4.5 M20 20 V16 H16",
        "plus": "M12 5 V19 M5 12 H19",
        "link": "M10 14 L14 10 M8.5 11.5 L6.5 13.5 a3 3 0 0 0 4 4 L12.5 15.5 M15.5 12.5 L17.5 10.5 a3 3 0 0 0 -4 -4 L11.5 8.5",
        "player": "M3 5 H21 V19 H3 Z M10 9 L15 12 L10 15 Z",
    ]

    private static let lock = NSLock()
    nonisolated(unsafe) private static var made: [String: Path] = [:]

    /// The path of an icon (an empty one for an unknown name).
    static func path(_ name: String) -> Path {
        lock.lock()
        defer { lock.unlock() }
        if let known = made[name] { return known }
        let path = SVGPath.read(drawings[name] ?? "")
        made[name] = path
        return path
    }
}

/// Reads the SVG path text Haven's icons use: M, L, H, V, A and Z, in
/// absolute (capital) or relative (small) form. Arcs are circular (equal
/// radii, no rotation), as in every Haven icon.
enum SVGPath {
    static func read(_ text: String) -> Path {
        var tokens = tokenize(text)[...]
        var path = Path()
        var current = CGPoint.zero
        var start = CGPoint.zero
        var command: Character = "M"

        func number() -> CGFloat? {
            guard case let .number(value)? = tokens.first else { return nil }
            tokens.removeFirst()
            return value
        }

        while !tokens.isEmpty {
            if case let .command(c) = tokens.first! {
                command = c
                tokens.removeFirst()
            }
            let relative = command.isLowercase
            func point() -> CGPoint? {
                guard let x = number(), let y = number() else { return nil }
                return relative ? CGPoint(x: current.x + x, y: current.y + y) : CGPoint(x: x, y: y)
            }
            switch command.uppercased().first! {
            case "M":
                guard let p = point() else { return path }
                path.move(to: p)
                current = p
                start = p
                // Further pairs after a move are lines.
                command = relative ? "l" : "L"
            case "L":
                guard let p = point() else { return path }
                path.addLine(to: p)
                current = p
            case "H":
                guard let x = number() else { return path }
                current = CGPoint(x: relative ? current.x + x : x, y: current.y)
                path.addLine(to: current)
            case "V":
                guard let y = number() else { return path }
                current = CGPoint(x: current.x, y: relative ? current.y + y : y)
                path.addLine(to: current)
            case "A":
                guard let radius = number(), number() != nil, number() != nil,
                      let large = number(), let sweep = number(), let end = point() else { return path }
                arc(&path, from: current, to: end, radius: radius, large: large != 0, sweep: sweep != 0)
                current = end
            case "Z":
                path.closeSubpath()
                current = start
                if case .number? = tokens.first { tokens.removeFirst() }
            default:
                return path
            }
        }
        return path
    }

    /// A circular arc from `a` to `b` (SVG's endpoint form, radii equal).
    static func arc(_ path: inout Path, from a: CGPoint, to b: CGPoint, radius: CGFloat, large: Bool, sweep: Bool) {
        guard radius > 0, a != b else {
            path.addLine(to: b)
            return
        }
        let hx = (a.x - b.x) / 2
        let hy = (a.y - b.y) / 2
        var r = radius
        let reach = (hx * hx + hy * hy) / (r * r)
        if reach > 1 { r *= reach.squareRoot() }
        let half = hx * hx + hy * hy
        let root = max(0, (r * r - half) / half).squareRoot() * (large == sweep ? -1 : 1)
        let center = CGPoint(x: root * hy + (a.x + b.x) / 2, y: -root * hx + (a.y + b.y) / 2)
        let from = atan2(a.y - center.y, a.x - center.x)
        let to = atan2(b.y - center.y, b.x - center.x)
        // In SVG (y down) sweep 1 turns clockwise on screen; SwiftUI's
        // `clockwise` is the other way round in y-down space.
        path.addArc(center: center, radius: r, startAngle: .radians(from), endAngle: .radians(to), clockwise: !sweep)
    }

    enum Token: Equatable {
        case command(Character)
        case number(CGFloat)
    }

    static func tokenize(_ text: String) -> [Token] {
        var tokens: [Token] = []
        var digits = ""
        func flush() {
            if let value = Double(digits) { tokens.append(.number(CGFloat(value))) }
            digits = ""
        }
        for c in text {
            if c.isLetter {
                flush()
                tokens.append(.command(c))
            } else if c == "-" {
                flush()
                digits = "-"
            } else if c.isNumber || c == "." {
                digits.append(c)
            } else {
                flush()
            }
        }
        flush()
        return tokens
    }
}
