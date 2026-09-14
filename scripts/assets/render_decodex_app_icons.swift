#!/usr/bin/env swift
import AppKit
import CoreGraphics
import Foundation

// Shared mark geometry owns both icon surfaces. Icon Composer owns Dock materials.
let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
let appIconGenerated = root.appendingPathComponent("assets/app-icon/generated")
let trayIconGenerated = root.appendingPathComponent("assets/tray-icon/generated")
let canvasSize = 1_024
enum Palette { static let black = NSColor.black }
for directory in [appIconGenerated, trayIconGenerated] {
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
}
enum TemplateMark {
	static let canvasScale: CGFloat = 1.06
	static let cloudScale: CGFloat = 0.97
	static let boltCenter = NSPoint(x: 766, y: 378)
	static let boltScale: CGFloat = 1.82
	static let promptCenter = NSPoint(x: 456, y: 504)
	static let promptOffset = NSSize(width: 18, height: 0)
	static let promptScale: CGFloat = 0.88
	static let promptWidth: CGFloat = 108
}

func currentCGContext() -> CGContext {
	guard let context = NSGraphicsContext.current?.cgContext else {
		preconditionFailure("icon drawing requires a graphics context")
	}

	return context
}

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

func roundedRect(_ rect: NSRect, radius: CGFloat) -> NSBezierPath {
	NSBezierPath(roundedRect: rect, xRadius: radius, yRadius: radius)
}

func strokePath(_ points: [NSPoint], color: NSColor, width: CGFloat, alpha: CGFloat = 1) {
	guard let first = points.first else { return }
	let path = NSBezierPath()
	path.lineWidth = width
	path.lineCapStyle = .round
	path.lineJoinStyle = .round
	path.move(to: first)
	for point in points.dropFirst() {
		path.line(to: point)
	}
	color.withAlphaComponent(alpha).setStroke()
	path.stroke()
}

func fillPolygon(_ points: [NSPoint], color: NSColor, alpha: CGFloat = 1) {
	guard let first = points.first else { return }
	let path = NSBezierPath()
	path.move(to: first)
	for point in points.dropFirst() {
		path.line(to: point)
	}
	path.close()
	color.withAlphaComponent(alpha).setFill()
	path.fill()
}

func templateBoltPoints(center: NSPoint, scale: CGFloat) -> [NSPoint] {
	[
		NSPoint(x: center.x + 48 * scale, y: center.y + 130 * scale),
		NSPoint(x: center.x - 88 * scale, y: center.y + 18 * scale),
		NSPoint(x: center.x - 16 * scale, y: center.y + 18 * scale),
		NSPoint(x: center.x - 56 * scale, y: center.y - 128 * scale),
		NSPoint(x: center.x + 96 * scale, y: center.y - 8 * scale),
		NSPoint(x: center.x + 20 * scale, y: center.y - 8 * scale),
	]
}

func cloudPath() -> NSBezierPath {
	let path = NSBezierPath()
	path.append(NSBezierPath(ovalIn: NSRect(x: 120, y: 372, width: 332, height: 332)))
	path.append(NSBezierPath(ovalIn: NSRect(x: 270, y: 448, width: 374, height: 374)))
	path.append(NSBezierPath(ovalIn: NSRect(x: 492, y: 338, width: 326, height: 326)))
	path.append(NSBezierPath(ovalIn: NSRect(x: 170, y: 244, width: 322, height: 322)))
	path.append(NSBezierPath(ovalIn: NSRect(x: 354, y: 232, width: 370, height: 370)))
	path.append(roundedRect(NSRect(x: 198, y: 280, width: 570, height: 360), radius: 170))
	return path
}

func promptCenterlines() -> [[NSPoint]] {
    [
        [NSPoint(x: 292, y: 590), NSPoint(x: 382, y: 506), NSPoint(x: 292, y: 422)],
        [NSPoint(x: 472, y: 418), NSPoint(x: 620, y: 418)],
    ]
}

func drawPromptMark(color: NSColor, width: CGFloat, alpha: CGFloat = 1) {
    for points in promptCenterlines() {
        strokePath(points, color: color, width: width, alpha: alpha)
    }
}

func drawTemplateBolt() {
	fillPolygon(templateBoltPoints(center: TemplateMark.boltCenter, scale: TemplateMark.boltScale), color: .black)
}

func drawTemplateCloud() {
	let context = currentCGContext()
	context.saveGState()
	context.translateBy(x: 512, y: 512)
	context.scaleBy(x: TemplateMark.cloudScale, y: TemplateMark.cloudScale)
	context.translateBy(x: -512, y: -512)
	Palette.black.setFill()
	cloudPath().fill()
	context.restoreGState()
}

func clearTemplatePrompt() {
	let context = currentCGContext()
	context.saveGState()
	context.setBlendMode(.clear)
	context.translateBy(x: TemplateMark.promptOffset.width, y: TemplateMark.promptOffset.height)
	context.translateBy(x: TemplateMark.promptCenter.x, y: TemplateMark.promptCenter.y)
	context.scaleBy(x: TemplateMark.promptScale, y: TemplateMark.promptScale)
	context.translateBy(x: -TemplateMark.promptCenter.x, y: -TemplateMark.promptCenter.y)
	drawPromptMark(color: .clear, width: TemplateMark.promptWidth)
	context.restoreGState()
}

func drawTemplateMark() {
	let context = currentCGContext()
	context.saveGState()
	context.translateBy(x: 512, y: 512)
	context.scaleBy(x: TemplateMark.canvasScale, y: TemplateMark.canvasScale)
	context.translateBy(x: -512, y: -512)
	drawTemplateBolt()
	drawTemplateCloud()
	clearTemplatePrompt()
	context.restoreGState()
}

func drawTrayIcon() throws -> NSBitmapImageRep {
	try bitmap { _ in
		drawTemplateMark()
	}
}

// Both icon surfaces use the menu-bar mark geometry. Dock changes only its
// canvas placement and material; SVG files are generated, never hand-edited.
func transformed(_ path: CGPath, _ transform: CGAffineTransform) -> CGPath {
    var transform = transform
    return path.copy(using: &transform)!
}

func sharedMarkPaths() -> [(String, CGPath, String)] {
    let cloudTransform = CGAffineTransform(translationX: 512, y: 512)
        .scaledBy(x: TemplateMark.cloudScale, y: TemplateMark.cloudScale)
        .translatedBy(x: -512, y: -512)
    let cloud = transformed(cloudPath().cgPath.normalized(), cloudTransform)
    let bolt = CGMutablePath()
    bolt.addLines(between: templateBoltPoints(center: TemplateMark.boltCenter, scale: TemplateMark.boltScale))
    bolt.closeSubpath()
    let promptTransform = CGAffineTransform(translationX: TemplateMark.promptOffset.width, y: TemplateMark.promptOffset.height)
        .translatedBy(x: TemplateMark.promptCenter.x, y: TemplateMark.promptCenter.y)
        .scaledBy(x: TemplateMark.promptScale, y: TemplateMark.promptScale)
        .translatedBy(x: -TemplateMark.promptCenter.x, y: -TemplateMark.promptCenter.y)
    let prompt = promptCenterlines().map { points -> CGPath in
        let line = CGMutablePath()
        line.addLines(between: points)
        return transformed(line.copy(strokingWithWidth: TemplateMark.promptWidth, lineCap: .round, lineJoin: .round, miterLimit: 10), promptTransform)
    }
    return [("cloud", cloud, "#dbeeff"), ("chevron", prompt[0], "#14314d"),
            ("cursor", prompt[1], "#14314d"), ("lightning", bolt, "#ffc247")]
}

func svgPath(_ path: CGPath) -> String {
    var parts: [String] = []
    func point(_ p: CGPoint) -> String { String(format: "%.3f %.3f", p.x, p.y) }
    path.applyWithBlock { element in
        let e = element.pointee
        switch e.type {
        case .moveToPoint: parts.append("M " + point(e.points[0]))
        case .addLineToPoint: parts.append("L " + point(e.points[0]))
        case .addQuadCurveToPoint: parts.append("Q " + point(e.points[0]) + " " + point(e.points[1]))
        case .addCurveToPoint: parts.append("C " + point(e.points[0]) + " " + point(e.points[1]) + " " + point(e.points[2]))
        case .closeSubpath: parts.append("Z")
        @unknown default: preconditionFailure("Unsupported vector element")
        }
    }
    return parts.joined(separator: " ")
}

func writeComposerComponents() throws {
    let components = sharedMarkPaths()
    let cloudBounds = components[0].1.boundingBoxOfPath
    // Anchor the cloud, not the combined cloud/lightning bounding box.
    let placement = CGAffineTransform(translationX: 512, y: 480)
        .scaledBy(x: 0.90, y: -0.90)
        .translatedBy(x: -cloudBounds.midX, y: -cloudBounds.midY)
    let directory = root.appendingPathComponent("assets/app-icon/composer/AppIcon.icon/Assets")
    for (name, path, color) in components {
        let shape = svgPath(transformed(path, placement))
        let svg = """
        <svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
          <!-- Generated from shared menu-bar geometry. -->
          <path d="\(shape)" fill="\(color)"/>
        </svg>

        """
        try svg.write(to: directory.appendingPathComponent("\(name).svg"), atomically: true, encoding: .utf8)
    }
}

try writeComposerComponents()

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
let temporary = FileManager.default.temporaryDirectory.appendingPathComponent("decodex-icon-\(UUID().uuidString)")
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
try writePNG(try drawTrayIcon(), to: trayIconGenerated.appendingPathComponent("tray-icon-template.png"))

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
