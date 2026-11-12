import AppKit
import Foundation

let directory = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
for size in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let pixels = size * scale
        let image = NSImage(size: NSSize(width: pixels, height: pixels))
        image.lockFocus()
        let dimension = CGFloat(pixels)
        let tile = NSRect(x: dimension * 0.04, y: dimension * 0.04, width: dimension * 0.92, height: dimension * 0.92)
        NSColor(calibratedRed: 0.09, green: 0.11, blue: 0.15, alpha: 1).setFill()
        NSBezierPath(roundedRect: tile, xRadius: dimension * 0.20, yRadius: dimension * 0.20).fill()
        let envelope = NSRect(x: dimension * 0.20, y: dimension * 0.28, width: dimension * 0.60, height: dimension * 0.43)
        NSColor(calibratedRed: 0.46, green: 0.73, blue: 1, alpha: 1).setFill()
        NSBezierPath(roundedRect: envelope, xRadius: dimension * 0.045, yRadius: dimension * 0.045).fill()
        NSColor(calibratedRed: 0.09, green: 0.11, blue: 0.15, alpha: 1).setStroke()
        let flap = NSBezierPath()
        flap.move(to: NSPoint(x: dimension * 0.21, y: dimension * 0.69))
        flap.line(to: NSPoint(x: dimension * 0.50, y: dimension * 0.47))
        flap.line(to: NSPoint(x: dimension * 0.79, y: dimension * 0.69))
        flap.lineWidth = max(1, dimension * 0.028)
        flap.stroke()
        image.unlockFocus()
        guard let tiff = image.tiffRepresentation,
              let bitmap = NSBitmapImageRep(data: tiff),
              let png = bitmap.representation(using: .png, properties: [:]) else {
            fatalError("Cannot encode application icon")
        }
        let suffix = scale == 2 ? "@2x" : ""
        try png.write(to: directory.appendingPathComponent("icon_\(size)x\(size)\(suffix).png"))
    }
}
