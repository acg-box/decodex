import AppKit
@testable import DecodexApp
import XCTest

final class LoginPanelLayoutTests: XCTestCase {
	func testPanelSizeRoundsUpAndKeepsMinimum() {
		let size = LoginPanelLayout.panelSize(for: NSSize(width: 328.2, height: 189.4))

		XCTAssertEqual(size.width, 329)
		XCTAssertEqual(size.height, 190)
	}

	func testExistingPanelKeepsCurrentTopWhenParentWindowIsMissing() {
		let currentFrame = NSRect(x: 300, y: 200, width: 80, height: 120)
		let origin = LoginPanelLayout.origin(
			for: NSSize(width: 100, height: 50),
			parentFrame: nil,
			currentFrame: currentFrame,
			mouseLocation: NSPoint(x: 20, y: 20),
			visibleFrame: NSRect(x: 0, y: 0, width: 1_000, height: 1_000)
		)

		XCTAssertEqual(origin.x, 290)
		XCTAssertEqual(origin.y, 270)
	}

	func testParentWindowPlacementOverridesMouseAndCurrentFrame() {
		let origin = LoginPanelLayout.origin(
			for: NSSize(width: 120, height: 80),
			parentFrame: NSRect(x: 100, y: 100, width: 400, height: 300),
			currentFrame: NSRect(x: 800, y: 800, width: 100, height: 100),
			mouseLocation: NSPoint(x: 20, y: 20),
			visibleFrame: NSRect(x: 0, y: 0, width: 1_000, height: 1_000)
		)

		XCTAssertEqual(origin.x, 240)
		XCTAssertEqual(origin.y, 252)
	}

	func testOriginIsClampedInsideVisibleFrame() {
		let origin = LoginPanelLayout.origin(
			for: NSSize(width: 200, height: 100),
			parentFrame: nil,
			currentFrame: nil,
			mouseLocation: NSPoint(x: 10, y: 10),
			visibleFrame: NSRect(x: 0, y: 0, width: 300, height: 200)
		)

		XCTAssertEqual(origin.x, 8)
		XCTAssertEqual(origin.y, 8)
	}

	func testSizeDiffUsesTolerance() {
		XCTAssertFalse(
			LoginPanelLayout.sizeDiffers(
				NSSize(width: 328, height: 190),
				NSSize(width: 328.4, height: 190.5)
			)
		)
		XCTAssertTrue(
			LoginPanelLayout.sizeDiffers(
				NSSize(width: 328, height: 190),
				NSSize(width: 328.6, height: 190)
			)
		)
	}
}
