import AppKit
import SwiftUI
@testable import DecodexApp
import XCTest

final class ThinMaterialViewTests: XCTestCase {
	@MainActor
	func testBridgeKeepsContentAndMaterialInSeparateViews() {
		let surface = ThinMaterialView(frame: NSRect(x: 0, y: 0, width: 300, height: 50))
		let foreground = NSView(frame: .zero)
		surface.cornerRadius = 14
		surface.contentView = foreground
		XCTAssertTrue(NSClassFromString("DecodexThinMaterialView") === ThinMaterialView.self)
		XCTAssertFalse(surface.isOpaque)
		XCTAssertFalse(surface.subviews.first!.isOpaque, "The material host must match the menu-bar host")
		XCTAssertEqual(surface.subviews.count, 2)
		XCTAssertTrue(surface.subviews.last === foreground)
		XCTAssertEqual(foreground.frame, surface.bounds)
		XCTAssertEqual(surface.layer?.cornerRadius, 14)
		surface.setFrameSize(NSSize(width: 440, height: 80))
		XCTAssertEqual(foreground.frame, surface.bounds)
		XCTAssertTrue(surface.hitTest(NSPoint(x: 20, y: 20)) === foreground)
	}

	@MainActor
	func testMatteBackdropIsOpaqueAndTracksAppearance() throws {
		let surface = ThinMaterialView(frame: NSRect(x: 0, y: 0, width: 300, height: 50))
		var colors: [CGColor] = []
		for name in [NSAppearance.Name.aqua, .darkAqua] {
			surface.appearance = NSAppearance(named: name)
			surface.viewDidChangeEffectiveAppearance()
			let color = try XCTUnwrap(surface.layer?.backgroundColor)
			XCTAssertEqual(color.alpha, 1)
			colors.append(color)
		}
		XCTAssertNotEqual(colors[0], colors[1])
	}

	@MainActor
	func testReplacingMaterialKeepsTheSameForeground() {
		weak var previous: ThinMaterialView?
		let foreground = NSView(frame: .zero)
		let next = ThinMaterialView(frame: NSRect(x: 0, y: 0, width: 300, height: 50))
		autoreleasepool {
			let first = ThinMaterialView(frame: NSRect(x: 0, y: 0, width: 300, height: 50))
			previous = first
			first.contentView = foreground
			first.contentView = nil
			next.contentView = foreground
		}
		XCTAssertNil(previous)
		XCTAssertTrue(foreground.superview === next)
		XCTAssertEqual(foreground.frame, next.bounds)
	}
}
