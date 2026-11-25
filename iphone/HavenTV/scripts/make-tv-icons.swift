// Makes the Apple TV icon and Top Shelf pictures from Haven's halo:
//   swift iphone/HavenTV/scripts/make-tv-icons.swift <Halo.png> <Brand.brandassets folder>
// The icon has two layers (dark back with a gold glow, the halo in front)
// so tvOS can give it its parallax.
import AppKit

let args = CommandLine.arguments
guard args.count == 3, let halo = NSImage(contentsOfFile: args[1]) else {
    print("usage: make-tv-icons.swift Halo.png Out.brandassets"); exit(2)
}
let out = URL(fileURLWithPath: args[2])
let fm = FileManager.default
try? fm.removeItem(at: out)
let dark = NSColor(srgbRed: 0x0d / 255, green: 0x0f / 255, blue: 0x14 / 255, alpha: 1)
let gold = NSColor(srgbRed: 0xe8 / 255, green: 0xb9 / 255, blue: 0x4a / 255, alpha: 1)

func png(_ w: Int, _ h: Int, _ draw: (CGRect) -> Void) -> Data {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h, bitsPerSample: 8,
                               samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                               colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    draw(CGRect(x: 0, y: 0, width: w, height: h))
    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

func back(_ r: CGRect) {
    dark.setFill(); r.fill()
    let glow = NSGradient(colors: [gold.withAlphaComponent(0.35), gold.withAlphaComponent(0)])!
    glow.draw(fromCenter: CGPoint(x: r.midX, y: r.midY), radius: 0,
              toCenter: CGPoint(x: r.midX, y: r.midY), radius: min(r.width, r.height) * 0.55, options: [])
}

func front(_ r: CGRect, share: CGFloat = 0.72) {
    let side = min(r.width, r.height) * share
    halo.draw(in: CGRect(x: r.midX - side / 2, y: r.midY - side / 2, width: side, height: side))
}

func write(_ folder: URL, _ name: String, _ data: Data) {
    try! fm.createDirectory(at: folder, withIntermediateDirectories: true)
    try! data.write(to: folder.appendingPathComponent(name))
}

func json(_ folder: URL, _ object: Any) {
    try! fm.createDirectory(at: folder, withIntermediateDirectories: true)
    let data = try! JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys])
    try! data.write(to: folder.appendingPathComponent("Contents.json"))
}

let info: [String: Any] = ["author": "xcode", "version": 1]

/// An image stack with a back and a front layer, at the given sizes (scale: pixels).
func stack(_ name: String, _ sizes: [(String, Int, Int)]) {
    let folder = out.appendingPathComponent("\(name).imagestack")
    json(folder, ["info": info, "layers": [["filename": "Front.imagestacklayer"], ["filename": "Back.imagestacklayer"]]])
    for (layer, drawer) in [("Front", { (r: CGRect) in front(r) }), ("Back", { (r: CGRect) in back(r) })] {
        let layerFolder = folder.appendingPathComponent("\(layer).imagestacklayer")
        json(layerFolder, ["info": info])
        let set = layerFolder.appendingPathComponent("Content.imageset")
        var images: [[String: Any]] = []
        for (scale, w, h) in sizes {
            let file = "\(layer.lowercased())-\(scale).png"
            write(set, file, png(w, h, drawer))
            images.append(["idiom": "tv", "filename": file, "scale": scale])
        }
        json(set, ["info": info, "images": images])
    }
}

func flat(_ name: String, _ sizes: [(String, Int, Int)]) {
    let set = out.appendingPathComponent("\(name).imageset")
    var images: [[String: Any]] = []
    for (scale, w, h) in sizes {
        let file = "shelf-\(scale).png"
        write(set, file, png(w, h) { r in back(r); front(r, share: 0.6) })
        images.append(["idiom": "tv", "filename": file, "scale": scale])
    }
    json(set, ["info": info, "images": images])
}

stack("App Icon", [("1x", 400, 240), ("2x", 800, 480)])
stack("App Icon - App Store", [("1x", 1280, 768)])
flat("Top Shelf Image", [("1x", 1920, 720), ("2x", 3840, 1440)])
flat("Top Shelf Image Wide", [("1x", 2320, 720), ("2x", 4640, 1440)])
json(out, ["info": info, "assets": [
    ["filename": "App Icon - App Store.imagestack", "idiom": "tv", "role": "primary-app-icon", "size": "1280x768"],
    ["filename": "App Icon.imagestack", "idiom": "tv", "role": "primary-app-icon", "size": "400x240"],
    ["filename": "Top Shelf Image Wide.imageset", "idiom": "tv", "role": "top-shelf-image-wide", "size": "2320x720"],
    ["filename": "Top Shelf Image.imageset", "idiom": "tv", "role": "top-shelf-image", "size": "1920x720"],
]])
print("Made \(out.path)")
