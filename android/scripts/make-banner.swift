// Makes Haven's Android TV banner (320x180, the launcher's row picture)
// from the halo, in the Apple TV pictures' style: dark with a gold glow,
// the halo on the left and "Haven" beside it.
//   swift android/scripts/make-banner.swift app/assets/icon/haven.png android/app/src/main/res/drawable-xhdpi/banner.png
import AppKit

let args = CommandLine.arguments
guard args.count == 3, let halo = NSImage(contentsOfFile: args[1]) else {
    print("usage: make-banner.swift haven.png banner.png"); exit(2)
}
let dark = NSColor(srgbRed: 0x0d / 255, green: 0x0f / 255, blue: 0x14 / 255, alpha: 1)
let gold = NSColor(srgbRed: 0xe8 / 255, green: 0xb9 / 255, blue: 0x4a / 255, alpha: 1)
let (w, h) = (320, 180)
let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h, bitsPerSample: 8,
                           samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                           colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
let r = CGRect(x: 0, y: 0, width: w, height: h)
dark.setFill(); r.fill()
let centre = CGPoint(x: 92, y: r.midY)
NSGradient(colors: [gold.withAlphaComponent(0.32), gold.withAlphaComponent(0)])!
    .draw(fromCenter: centre, radius: 0, toCenter: centre, radius: 110, options: [])
let side: CGFloat = 132
halo.draw(in: CGRect(x: centre.x - side / 2, y: centre.y - side / 2, width: side, height: side))
let text = NSAttributedString(string: "Haven", attributes: [
    .font: NSFont.systemFont(ofSize: 46, weight: .bold),
    .foregroundColor: NSColor.white,
])
let size = text.size()
text.draw(at: CGPoint(x: 166, y: r.midY - size.height / 2))
NSGraphicsContext.restoreGraphicsState()
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: args[2]))
print("Made \(args[2])")
