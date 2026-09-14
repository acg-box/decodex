#!/usr/bin/env swift
import AppKit
import Foundation

// Geometry and materials have one owner; this exports the chosen default.
let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
let appIconGenerated = root.appendingPathComponent("assets/app-icon/generated")
let trayIconGenerated = root.appendingPathComponent("assets/tray-icon/generated")
let canvasSize = 1024

func bitmap(size: Int = canvasSize, drawing: (CGContext) -> Void) throws -> NSBitmapImageRep {
	guard let rep = NSBitmapImageRep(
		bitmapDataPlanes: nil,
		pixelsWide: size,
		pixelsHigh: size,
		bitsPerSample: 8,
		samplesPerPixel: 4,
		hasAlpha: true,
		isPlanar: false,
		colorSpaceName: .deviceRGB,
		bytesPerRow: 0,
		bitsPerPixel: 0
	) else {
		throw NSError(domain: "DecodexIconRender", code: 1)
	}

	NSGraphicsContext.saveGraphicsState()
	defer {
		NSGraphicsContext.restoreGraphicsState()
	}
	guard let graphicsContext = NSGraphicsContext(bitmapImageRep: rep) else {
		throw NSError(domain: "DecodexIconRender", code: 3)
	}
	NSGraphicsContext.current = graphicsContext
	let context = graphicsContext.cgContext
	context.setShouldAntialias(true)
	context.setAllowsAntialiasing(true)
	context.interpolationQuality = .high
	drawing(context)

	return rep
}
func writePNG(_ rep: NSBitmapImageRep, to url: URL) throws {
	guard let data = rep.representation(using: .png, properties: [:]) else {
		throw NSError(domain: "DecodexIconRender", code: 2)
	}
	try data.write(to: url)
}
func run(_ executable: String, _ arguments: [String]) throws {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: executable)
    process.arguments = arguments
    try process.run()
    process.waitUntilExit()
    guard process.terminationStatus == 0 else {
        throw NSError(domain: "DecodexIconRender", code: Int(process.terminationStatus))
    }
}

try run("/usr/bin/env", ["swift", root.appendingPathComponent("scripts/assets/build_liquid_glass_icons.swift").path])
let variant = try String(contentsOf: root.appendingPathComponent("assets/app-icon/default-variant"), encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
precondition(["01-mercury-cloud", "02-flat-cloud", "03-open-cloud"].contains(variant))
for directory in [appIconGenerated, trayIconGenerated] {
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
}
let temporary = FileManager.default.temporaryDirectory.appendingPathComponent("decodex-default-icon-\(UUID().uuidString)")
try FileManager.default.createDirectory(at: temporary, withIntermediateDirectories: true)
defer { try? FileManager.default.removeItem(at: temporary) }
try run(root.appendingPathComponent("scripts/macos/compile_decodex_app_icon.sh").path, [temporary.path])
let compiled = temporary.appendingPathComponent("AppIcon.icns")
try Data(contentsOf: compiled).write(to: appIconGenerated.appendingPathComponent("app-icon.icns"))
let iconset = temporary.appendingPathComponent("Preview.iconset")
try run("/usr/bin/iconutil", ["-c", "iconset", compiled.path, "-o", iconset.path])
let preview = try Data(contentsOf: iconset.appendingPathComponent("icon_128x128@2x.png"))
try preview.write(to: appIconGenerated.appendingPathComponent("app-icon-default-preview.png"))
try preview.write(to: appIconGenerated.appendingPathComponent("app-icon-flat.png"))
try Data(contentsOf: root.appendingPathComponent("assets/app-icon/liquid-glass/\(variant)/StatusBarIcon.png")).write(to: trayIconGenerated.appendingPathComponent("tray-icon-template.png"))
// Review the compiled fallback at Dock sizes on two backgrounds.
let review = try bitmap(size: 768) { _ in
    let icon = NSImage(contentsOf: compiled)!
    for (row, background) in [NSColor(calibratedWhite: 0.94, alpha: 1),
                              NSColor(calibratedWhite: 0.10, alpha: 1)].enumerated() {
        let bottom = CGFloat(1 - row) * 384
        background.setFill()
        NSRect(x: 0, y: bottom, width: 768, height: 384).fill()
        let foreground = row == 0 ? NSColor.darkGray : NSColor.lightGray
        let attributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 14), .foregroundColor: foreground,
        ]
        (row == 0 ? "LIGHT BACKGROUND" : "DARK BACKGROUND" as NSString).draw(
            at: NSPoint(x: 32, y: bottom + 345), withAttributes: attributes)
        for (index, size) in [32, 64, 128, 256].enumerated() {
            let center: CGFloat = [68, 186, 344, 588][index]
            let edge = CGFloat(size)
            icon.draw(in: NSRect(x: center - edge / 2, y: bottom + 180 - edge / 2,
                                 width: edge, height: edge))
            ("\(size) px" as NSString).draw(at: NSPoint(x: center - 23, y: bottom + 25),
                                          withAttributes: attributes)
        }
    }
}
try writePNG(review, to: appIconGenerated.appendingPathComponent("app-icon-size-review.png"))
