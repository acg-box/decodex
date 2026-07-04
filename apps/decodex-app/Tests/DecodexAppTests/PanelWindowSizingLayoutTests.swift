import AppKit
@testable import DecodexApp
import XCTest

final class PanelWindowSizingLayoutTests: XCTestCase {
	func testRoundedContentSizeCeilsFractionalDimensions() {
		let size = PanelWindowSizingLayout.roundedContentSize(for: CGSize(width: 339.2, height: 641.1))

		XCTAssertEqual(size, NSSize(width: 340, height: 642))
	}

	func testFrameKeepsTopEdgeAndCenterWhenContentShrinks() {
		let currentFrame = NSRect(x: 120, y: 200, width: 360, height: 700)
		let frame = PanelWindowSizingLayout.frame(
			forContentSize: NSSize(width: 322, height: 520),
			currentFrame: currentFrame
		) { contentSize in
			NSSize(width: contentSize.width + 18, height: contentSize.height + 18)
		} visibleFrame: {
			NSRect(x: 0, y: 0, width: 1_000, height: 1_000)
		}

		XCTAssertEqual(frame.width, 340)
		XCTAssertEqual(frame.height, 538)
		XCTAssertEqual(frame.midX, currentFrame.midX)
		XCTAssertEqual(frame.maxY, currentFrame.maxY)
	}

	func testFrameClampsInsideVisibleFrameWhenContentGrowsNearScreenEdge() {
		let frame = PanelWindowSizingLayout.frame(
			forContentSize: NSSize(width: 420, height: 360),
			currentFrame: NSRect(x: 700, y: 620, width: 340, height: 220)
		) { contentSize in
			contentSize
		} visibleFrame: {
			NSRect(x: 0, y: 0, width: 800, height: 700)
		}

		XCTAssertEqual(frame.minX, 800 - 420 - 8)
		XCTAssertEqual(frame.maxY, 700 - 8)
	}

	func testOversizedFrameFallsBackToVisibleFrameLeadingMargin() {
		let frame = PanelWindowSizingLayout.frame(
			forContentSize: NSSize(width: 900, height: 760),
			currentFrame: NSRect(x: 100, y: 100, width: 340, height: 220)
		) { contentSize in
			contentSize
		} visibleFrame: {
			NSRect(x: 0, y: 0, width: 800, height: 700)
		}

		XCTAssertEqual(frame.minX, 8)
		XCTAssertEqual(frame.minY, 8)
	}
}
