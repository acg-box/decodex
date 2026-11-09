#!/usr/bin/env swift
// Render the native macOS SF Symbols used by the GPUI workspace toolbar.
import AppKit

let output = URL(fileURLWithPath: CommandLine.arguments.dropFirst().first ?? "assets/workspace-symbols", isDirectory: true)
try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
let light = CommandLine.arguments.contains("--light")
let symbols = [
    "activity-work": "waveform.path",
    "activity-performance": "gauge.open.with.lines.needle.33percent",
    "activity-goal": "scope",
    "activity-search": "magnifyingglass",
    "activity-edit": "pencil",
    "activity-command": "terminal",
    "activity-handoff": "arrow.turn.up.right",
    "team-members": "person.2.fill",
    "team-parent": "arrow.turn.up.left",
    "document": "doc.text",
    "link": "link",
    "account-route-active": "point.topleft.down.to.point.bottomright.curvepath",
    "power-on": "power",
    "power-off": "power",
    "lock": "lock.fill",
    "shield": "shield",
    "shield-check": "checkmark.shield",
    "shield-slash": "shield.slash",
    "shield-lock": "lock.shield",
    "eye": "eye",
    "eye-slash": "eye.slash",
    "account-route": "point.topleft.down.to.point.bottomright.curvepath",
    "fork": "arrow.triangle.branch",
    "account-sign-in": "person.crop.circle.badge.plus",
    "reset-cards": "arrow.clockwise",
    "account-warning": "exclamationmark.circle",
    "account-warning-amber": "exclamationmark.circle",
    "account-reorder": "line.3.horizontal",
    "arrange": "square.grid.2x2",
    "fit": "viewfinder",
    "all-work": "square.stack.3d.up",
    "account-logout": "rectangle.portrait.and.arrow.right",
    "confirm": "checkmark",
    "sidebar": "sidebar.left",
    "graph": "rectangle.topthird.inset.filled",
    "timeline": "clock",
    "agents": "sidebar.right",
    "expand": "arrow.up.left.and.arrow.down.right",
    "restore": "arrow.down.right.and.arrow.up.left",
    "archive": "archivebox",
    "unarchive": "tray.and.arrow.up",
    "trash": "trash",
    "settings": "gearshape",
    "close": "xmark",
    "plus": "plus",
    "minus": "minus",
    "back": "arrow.left",
    "forward": "arrow.right",
    "send": "arrow.up",
    "fast": "bolt.fill",
    "ultrafast": "bolt.fill",
    "microphone": "mic",
    "waveform": "waveform",
    "bell": "bell",
    "arrow-down": "arrow.down",
    "bell-attention": "bell",
    "bell-info": "bell",
    "bell-error": "bell",
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
    if name == "ultrafast" {
        // Match the supplied Codex double-bolt silhouette, with a filled front and outlined rear.
        let path = NSBezierPath()
        path.move(to: NSPoint(x: 9.81527, y: 14.14515))
        path.curve(to: NSPoint(x: 11.19600, y: 13.30708), controlPoint1: NSPoint(x: 10.41935, y: 14.68307), controlPoint2: NSPoint(x: 11.39375, y: 14.12401))
        path.line(to: NSPoint(x: 10.46305, y: 10.28293))
        path.line(to: NSPoint(x: 13.86429, y: 10.28293))
        path.curve(to: NSPoint(x: 14.51273, y: 8.89017), controlPoint1: NSPoint(x: 14.58436, y: 10.28273), controlPoint2: NSPoint(x: 14.97615, y: 9.44116))
        path.line(to: NSPoint(x: 8.90764, y: 2.23389))
        path.curve(to: NSPoint(x: 8.22582, y: 2.17571), controlPoint1: NSPoint(x: 8.73556, y: 2.02975), controlPoint2: NSPoint(x: 8.43011, y: 2.00371))
        path.curve(to: NSPoint(x: 8.16756, y: 2.85753), controlPoint1: NSPoint(x: 8.02160, y: 2.34778), controlPoint2: NSPoint(x: 7.99556, y: 2.65324))
        path.line(to: NSPoint(x: 13.60647, y: 9.31560))
        path.line(to: NSPoint(x: 11.78618, y: 9.31560))
        path.curve(to: NSPoint(x: 11.56676, y: 8.74032), controlPoint1: NSPoint(x: 11.78109, y: 9.11707), controlPoint2: NSPoint(x: 11.71411, y: 8.91544))
        path.line(to: NSPoint(x: 5.99500, y: 2.12385))
        path.curve(to: NSPoint(x: 4.52340, y: 2.86960), controlPoint1: NSPoint(x: 5.41863, y: 1.43942), controlPoint2: NSPoint(x: 4.31259, y: 2.00000))
        path.line(to: NSPoint(x: 5.21375, y: 5.71760))
        path.line(to: NSPoint(x: 2.00778, y: 5.71760))
        path.curve(to: NSPoint(x: 1.28974, y: 7.26022), controlPoint1: NSPoint(x: 1.21009, y: 5.71760), controlPoint2: NSPoint(x: 0.77592, y: 6.65004))
        path.line(to: NSPoint(x: 6.86148, y: 13.87668))
        path.curve(to: NSPoint(x: 8.33309, y: 13.13095), controlPoint1: NSPoint(x: 7.43783, y: 14.56080), controlPoint2: NSPoint(x: 8.54356, y: 14.00033))
        path.line(to: NSPoint(x: 7.64272, y: 10.28293))
        path.line(to: NSPoint(x: 9.46727, y: 10.28293))
        path.line(to: NSPoint(x: 10.14982, y: 13.09828))
        path.line(to: NSPoint(x: 10.01985, y: 12.96049))
        path.curve(to: NSPoint(x: 9.33593, y: 12.94202), controlPoint1: NSPoint(x: 9.83607, y: 12.76663), controlPoint2: NSPoint(x: 9.52975, y: 12.75828))
        path.curve(to: NSPoint(x: 9.31818, y: 13.62526), controlPoint1: NSPoint(x: 9.14225, y: 13.12579), controlPoint2: NSPoint(x: 9.13447, y: 13.43148))
        path.line(to: NSPoint(x: 9.75775, y: 14.08975))
        path.line(to: NSPoint(x: 9.81527, y: 14.14515))
        path.close()
        NSColor.white.setFill()
        path.fill()
    } else {
        symbol.draw(in: NSRect(x: (16-size.width)/2, y: (16-size.height)/2, width: size.width, height: size.height),
            from: .zero, operation: .sourceOver, fraction: 1)
    }
    // Keep status tints equal to ui_theme::{ERROR, AMBER, BLUE}.
    let lightColor: NSColor = name == "send"
        ? NSColor(srgbRed: 1, green: 1, blue: 1, alpha: 1)
        : name == "account-warning" || name == "bell-error"
            ? NSColor(srgbRed: 200.0/255, green: 50.0/255, blue: 50.0/255, alpha: 1)
        : name == "bell-attention" || name == "account-warning-amber"
            ? NSColor(srgbRed: 149.0/255, green: 96.0/255, blue: 24.0/255, alpha: 1)
        : name == "bell-info" || name == "account-route-active" || name == "power-on"
            ? NSColor(srgbRed: 59.0/255, green: 96.0/255, blue: 187.0/255, alpha: 1)
        : NSColor(srgbRed: 63.0/255, green: 61.0/255, blue: 71.0/255, alpha: 1)
    (light ? lightColor : name == "send" ? NSColor(srgbRed: 0.04, green: 0.04, blue: 0.06, alpha: 1)
        : name == "account-warning" || name == "bell-error" ? NSColor(srgbRed: 239.0/255, green: 68.0/255, blue: 68.0/255, alpha: 1)
        : name == "bell-attention" || name == "account-warning-amber" ? NSColor(srgbRed: 224.0/255, green: 181.0/255, blue: 111.0/255, alpha: 1)
        : name == "bell-info" || name == "account-route-active" || name == "power-on" ? NSColor(srgbRed: 139.0/255, green: 170.0/255, blue: 247.0/255, alpha: 1)
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
