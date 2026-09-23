#!/usr/bin/env swift
// Render the native macOS SF Symbols used by the GPUI workspace toolbar.
import AppKit

let output = URL(fileURLWithPath: CommandLine.arguments.dropFirst().first ?? "assets/workspace-symbols", isDirectory: true)
try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
let symbols = [
    "account-route": "arrow.triangle.branch",
    "reset-card": "arrow.counterclockwise.circle",
    "account-info": "info.circle",
    "account-login": "arrow.clockwise",
    "account-logout": "rectangle.portrait.and.arrow.right",
    "confirm": "checkmark",
    "sidebar": "sidebar.left",
    "graph": "rectangle.bottomthird.inset.filled",
    "timeline": "clock",
    "agents": "sidebar.right",
    "expand": "arrow.up.left.and.arrow.down.right",
    "settings": "gearshape",
    "close": "xmark",
    "plus": "plus",
    "minus": "minus",
    "back": "arrow.left",
    "forward": "arrow.right",
    "send": "arrow.up",
    "fast": "bolt.fill",
    "voice": "waveform",
    "microphone": "mic",
    "bell": "bell",
    "arrow-down": "arrow.down",
    "bell-attention": "bell",
    "bell-info": "bell",
    "chevron-down": "chevron.down",
]
for (name, symbolName) in symbols {
    guard let symbol = NSImage(systemSymbolName: symbolName, accessibilityDescription: nil)?
        .withSymbolConfiguration(NSImage.SymbolConfiguration(pointSize: 14, weight: .regular)) else {
        fatalError("System symbol unavailable: \(symbolName)")
    }
    let pixels = 48
    guard let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0),
        let context = NSGraphicsContext(bitmapImageRep: bitmap) else {fatalError("Cannot allocate symbol bitmap")}
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = context
    context.cgContext.scaleBy(x: 3, y: 3)
    let scale = min(14 / symbol.size.width, 14 / symbol.size.height)
    let size = NSSize(width: symbol.size.width * scale, height: symbol.size.height * scale)
    symbol.draw(in: NSRect(x: (16-size.width)/2, y: (16-size.height)/2, width: size.width, height: size.height),
        from: .zero, operation: .sourceOver, fraction: 1)
    (name == "send" ? NSColor(srgbRed: 0.04, green: 0.04, blue: 0.06, alpha: 1)
        : name == "bell-attention" ? NSColor(srgbRed: 0.88, green: 0.70, blue: 0.40, alpha: 1)
        : name == "bell-info" ? NSColor(srgbRed: 0.54, green: 0.64, blue: 0.91, alpha: 1)
        : NSColor(srgbRed: 0.88, green: 0.86, blue: 0.90, alpha: 1)).setFill()
    NSRect(x: 0, y: 0, width: 16, height: 16).fill(using: .sourceAtop)
    NSGraphicsContext.restoreGraphicsState()
    // Center the visible ink, including symbols whose AppKit layout box has bearings.
    guard let bytes = bitmap.bitmapData else { fatalError("Missing bitmap storage") }
    let stride = bitmap.bytesPerRow
    let source = Array(UnsafeBufferPointer(start: bytes, count: stride * pixels))
    var left = pixels, right = -1, top = pixels, bottom = -1
    for y in 0..<pixels {
        for x in 0..<pixels where source[y * stride + x * 4 + 3] > 8 {
            left = min(left, x); right = max(right, x)
            top = min(top, y); bottom = max(bottom, y)
        }
    }
    guard right >= left else { fatalError("Empty symbol: \(symbolName)") }
    let dx = Int((Double(pixels - 1 - left - right) / 2).rounded())
    let dy = Int((Double(pixels - 1 - top - bottom) / 2).rounded())
    for index in 0..<(stride * pixels) { bytes[index] = 0 }
    for y in 0..<pixels {
        for x in 0..<pixels where (0..<pixels).contains(x + dx) && (0..<pixels).contains(y + dy) {
            for channel in 0..<4 {
                bytes[(y + dy) * stride + (x + dx) * 4 + channel] = source[y * stride + x * 4 + channel]
            }
        }
    }
    guard let data = bitmap.representation(using: .png, properties: [:]) else {fatalError("Cannot encode symbol")}
    try data.write(to: output.appendingPathComponent("\(name).png"))
}
