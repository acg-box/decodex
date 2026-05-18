#!/usr/bin/env swift

import AppKit
import CoreGraphics
import Foundation

let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
let appIconGenerated = root.appendingPathComponent("assets/app-icon/generated")
let appIconComposerAssets = root.appendingPathComponent("assets/app-icon/composer/AppIcon.icon/Assets")
let trayIconGenerated = root.appendingPathComponent("assets/tray-icon/generated")

for directory in [appIconGenerated, appIconComposerAssets, trayIconGenerated] {
	try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
}

let canvasSize = 1024
let appIconURL = appIconGenerated.appendingPathComponent("app-icon-flat.png")
let previewURL = appIconGenerated.appendingPathComponent("app-icon-default-preview.png")
let composerLayerURL = appIconComposerAssets.appendingPathComponent("app-icon-composer-layer.png")
let trayIconURL = trayIconGenerated.appendingPathComponent("tray-icon-template.png")
let icnsURL = appIconGenerated.appendingPathComponent("app-icon.icns")

enum Palette {
	static let fieldTop = NSColor(calibratedRed: 0.110, green: 0.145, blue: 0.230, alpha: 1)
	static let fieldBottom = NSColor(calibratedRed: 0.030, green: 0.050, blue: 0.090, alpha: 1)
	static let cloudTop = NSColor(calibratedRed: 0.965, green: 0.985, blue: 1.000, alpha: 1)
	static let cloudBottom = NSColor(calibratedRed: 0.700, green: 0.760, blue: 0.845, alpha: 1)
	static let ink = NSColor(calibratedRed: 0.155, green: 0.175, blue: 0.230, alpha: 1)
	static let bolt = NSColor(calibratedRed: 1.000, green: 0.760, blue: 0.230, alpha: 1)
	static let boltCore = NSColor(calibratedRed: 1.000, green: 0.925, blue: 0.410, alpha: 1)
	static let white = NSColor(calibratedWhite: 1.0, alpha: 1)
	static let black = NSColor(calibratedWhite: 0.0, alpha: 1)
}

enum TemplateMark {
	static let canvasScale: CGFloat = 1.06
	static let cloudScale: CGFloat = 0.97
	static let boltCenter = NSPoint(x: 766, y: 378)
	static let boltScale: CGFloat = 1.82
	static let promptCenter = NSPoint(x: 456, y: 504)
	static let promptOffset = NSSize(width: 18, height: 0)
	static let promptScale: CGFloat = 0.88
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
	NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
	NSGraphicsContext.current?.cgContext.setShouldAntialias(true)
	NSGraphicsContext.current?.cgContext.setAllowsAntialiasing(true)
	NSGraphicsContext.current?.cgContext.interpolationQuality = .high
	drawing(NSGraphicsContext.current!.cgContext)
	NSGraphicsContext.restoreGraphicsState()

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

func fillRoundedRect(_ rect: NSRect, radius: CGFloat, color: NSColor, alpha: CGFloat = 1) {
	color.withAlphaComponent(alpha).setFill()
	roundedRect(rect, radius: radius).fill()
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

func boltPoints(center: NSPoint, scale: CGFloat) -> [NSPoint] {
	[
		NSPoint(x: center.x + 22 * scale, y: center.y + 138 * scale),
		NSPoint(x: center.x - 92 * scale, y: center.y + 18 * scale),
		NSPoint(x: center.x - 18 * scale, y: center.y + 18 * scale),
		NSPoint(x: center.x - 58 * scale, y: center.y - 142 * scale),
		NSPoint(x: center.x + 104 * scale, y: center.y - 12 * scale),
		NSPoint(x: center.x + 22 * scale, y: center.y - 12 * scale),
	]
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

func appBoltPoints(offsetX: CGFloat = 0, offsetY: CGFloat = 0) -> [NSPoint] {
	[
		NSPoint(x: 850 + offsetX, y: 592 + offsetY),
		NSPoint(x: 718 + offsetX, y: 426 + offsetY),
		NSPoint(x: 800 + offsetX, y: 426 + offsetY),
		NSPoint(x: 756 + offsetX, y: 268 + offsetY),
		NSPoint(x: 904 + offsetX, y: 444 + offsetY),
		NSPoint(x: 832 + offsetX, y: 444 + offsetY),
	]
}

func appBoltCorePoints() -> [NSPoint] {
	[
		NSPoint(x: 838, y: 542),
		NSPoint(x: 764, y: 438),
		NSPoint(x: 808, y: 438),
		NSPoint(x: 784, y: 334),
		NSPoint(x: 850, y: 436),
		NSPoint(x: 814, y: 436),
	]
}

func drawTile() {
	let tile = roundedRect(NSRect(x: 58, y: 58, width: 908, height: 908), radius: 222)
	NSGradient(colors: [Palette.fieldTop, Palette.fieldBottom])!.draw(in: tile, angle: -48)

	Palette.white.withAlphaComponent(0.11).setStroke()
	let rim = roundedRect(NSRect(x: 82, y: 82, width: 860, height: 860), radius: 198)
	rim.lineWidth = 4
	rim.stroke()
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

func drawCloudContainer() {
	let shadow = NSShadow()
	shadow.shadowBlurRadius = 42
	shadow.shadowOffset = NSSize(width: 0, height: -24)
	shadow.shadowColor = NSColor(calibratedWhite: 0, alpha: 0.30)
	shadow.set()
	Palette.black.withAlphaComponent(0.24).setFill()
	cloudPath().fill()
	NSShadow().set()

	NSGradient(colors: [Palette.cloudTop, Palette.cloudBottom])!.draw(in: cloudPath(), angle: -58)
}

func drawPromptMark(color: NSColor, width: CGFloat, alpha: CGFloat = 1) {
	strokePath(
		[NSPoint(x: 292, y: 590), NSPoint(x: 382, y: 506), NSPoint(x: 292, y: 422)],
		color: color,
		width: width,
		alpha: alpha
	)
	strokePath(
		[NSPoint(x: 472, y: 418), NSPoint(x: 620, y: 418)],
		color: color,
		width: width,
		alpha: alpha
	)
}

func drawAppBolt() {
	fillPolygon(appBoltPoints(), color: Palette.bolt)
	fillPolygon(appBoltCorePoints(), color: Palette.boltCore, alpha: 0.54)
}

func drawAppMark() {
	drawCloudContainer()
	drawAppBolt()
	drawPromptMark(color: Palette.ink, width: 52)
}

func drawTemplateBolt() {
	fillPolygon(templateBoltPoints(center: TemplateMark.boltCenter, scale: TemplateMark.boltScale), color: .black)
}

func drawTemplateCloud() {
	let context = NSGraphicsContext.current!.cgContext
	context.saveGState()
	context.translateBy(x: 512, y: 512)
	context.scaleBy(x: TemplateMark.cloudScale, y: TemplateMark.cloudScale)
	context.translateBy(x: -512, y: -512)
	Palette.black.setFill()
	cloudPath().fill()
	context.restoreGState()
}

func clearTemplatePrompt() {
	let context = NSGraphicsContext.current!.cgContext
	context.saveGState()
	context.setBlendMode(.clear)
	context.translateBy(x: TemplateMark.promptOffset.width, y: TemplateMark.promptOffset.height)
	context.translateBy(x: TemplateMark.promptCenter.x, y: TemplateMark.promptCenter.y)
	context.scaleBy(x: TemplateMark.promptScale, y: TemplateMark.promptScale)
	context.translateBy(x: -TemplateMark.promptCenter.x, y: -TemplateMark.promptCenter.y)
	drawPromptMark(color: .clear, width: 108)
	context.restoreGState()
}

func drawTemplateMark() {
	let context = NSGraphicsContext.current!.cgContext
	context.saveGState()
	context.translateBy(x: 512, y: 512)
	context.scaleBy(x: TemplateMark.canvasScale, y: TemplateMark.canvasScale)
	context.translateBy(x: -512, y: -512)
	drawTemplateBolt()
	drawTemplateCloud()
	clearTemplatePrompt()
	context.restoreGState()
}

func drawAppIcon() throws -> NSBitmapImageRep {
	try bitmap { _ in
		drawTile()
		drawAppMark()
	}
}

func drawComposerLayer() throws -> NSBitmapImageRep {
	try bitmap { _ in
		drawAppMark()
	}
}

func drawTrayIcon() throws -> NSBitmapImageRep {
	try bitmap { _ in
		drawTemplateMark()
	}
}

func scaledPNG(from source: NSBitmapImageRep, size: Int, to url: URL) throws {
	let rep = try bitmap(size: size) { ctx in
		ctx.interpolationQuality = .high
		ctx.draw(source.cgImage!, in: CGRect(x: 0, y: 0, width: size, height: size))
	}
	try writePNG(rep, to: url)
}

func buildICNS(from source: NSBitmapImageRep) throws {
	let temp = URL(fileURLWithPath: NSTemporaryDirectory())
		.appendingPathComponent("decodex-app-icon-\(UUID().uuidString)")
	let iconset = temp.appendingPathComponent("AppIcon.iconset")
	try FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)

	let sizes: [(String, Int)] = [
		("icon_16x16.png", 16),
		("icon_16x16@2x.png", 32),
		("icon_32x32.png", 32),
		("icon_32x32@2x.png", 64),
		("icon_128x128.png", 128),
		("icon_128x128@2x.png", 256),
		("icon_256x256.png", 256),
		("icon_256x256@2x.png", 512),
		("icon_512x512.png", 512),
		("icon_512x512@2x.png", 1024),
	]
	for (name, size) in sizes {
		try scaledPNG(from: source, size: size, to: iconset.appendingPathComponent(name))
	}

	let process = Process()
	process.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
	process.arguments = ["-c", "icns", iconset.path, "-o", icnsURL.path]
	try process.run()
	process.waitUntilExit()
	if process.terminationStatus != 0 {
		throw NSError(domain: "DecodexIconRender", code: Int(process.terminationStatus))
	}
	try FileManager.default.removeItem(at: temp)
}

let appIcon = try drawAppIcon()
try writePNG(appIcon, to: appIconURL)
try scaledPNG(from: appIcon, size: 256, to: previewURL)
try buildICNS(from: appIcon)
try writePNG(try drawComposerLayer(), to: composerLayerURL)
try writePNG(try drawTrayIcon(), to: trayIconURL)
