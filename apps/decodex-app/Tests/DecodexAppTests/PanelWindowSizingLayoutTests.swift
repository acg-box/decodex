import AppKit
@testable import DecodexApp
import XCTest

final class PanelWindowSizingLayoutTests: XCTestCase {
	@MainActor
	func testPanelWindowHostIsTransparentBehindSystemGlass() {
		let window = NSWindow(
			contentRect: NSRect(x: 0, y: 0, width: 320, height: 480),
			styleMask: [.borderless],
			backing: .buffered,
			defer: false
		)
		window.isOpaque = true
		window.backgroundColor = .windowBackgroundColor

		PanelWindowAppearance.apply(to: window)

		XCTAssertFalse(window.isOpaque)
		XCTAssertEqual(window.backgroundColor, .clear)
	}

	@MainActor
	func testPanelWindowAppearanceIgnoresDetachedHost() {
		PanelWindowAppearance.apply(to: nil)
	}

	func testRoundedContentSizeCeilsFractionalDimensions() {
		let size = PanelWindowSizingLayout.roundedContentSize(for: CGSize(width: 339.2, height: 641.1))

		XCTAssertEqual(size, NSSize(width: 340, height: 642))
	}

	func testMeasuredAccountRowsHeightOverridesAnInflatedEstimate() {
		let height = AccountPanelLayout.resolvedAccountListContentHeight(
			measured: 612.2,
			estimated: 730
		)

		XCTAssertEqual(height, 613)
	}

	func testAccountRowsHeightUsesEstimateUntilMeasurementArrives() {
		XCTAssertEqual(
			AccountPanelLayout.resolvedAccountListContentHeight(
				measured: 0,
				estimated: 730
			),
			730
		)
		XCTAssertEqual(
			AccountPanelLayout.resolvedAccountListContentHeight(
				measured: .nan,
				estimated: 730
			),
			730
		)
	}

	func testHostingWindowScreenHeightOverridesPointerScreenFallback() {
		let height = AccountPanelLayout.resolvedScreenVisibleHeight(
			windowVisibleFrame: NSRect(x: 1_000, y: 0, width: 800, height: 620),
			fallback: 1_000
		)

		XCTAssertEqual(height, 620)
	}

	func testPointerScreenHeightIsFallbackBeforeWindowAttachment() {
		XCTAssertEqual(
			AccountPanelLayout.resolvedScreenVisibleHeight(
				windowVisibleFrame: nil,
				fallback: 760
			),
			760
		)
	}

	func testSixCompactAccountRowsUseTheirIntrinsicHeightOnASmallScreenWhenTheyFit() {
		let height = AccountPanelLayout.accountListHeight(
			accountCount: 6,
			measuredContentHeight: 432,
			windowVisibleFrame: NSRect(x: 0, y: 0, width: 800, height: 675)
		)

		XCTAssertEqual(height, 432)
	}

	func testSixCompactAccountRowsUseTheirFullHeightOnTheCurrentDisplay() {
		let height = AccountPanelLayout.accountListHeight(
			accountCount: 6,
			measuredContentHeight: 624,
			windowVisibleFrame: NSRect(x: 0, y: 0, width: 1_600, height: 1_350)
		)

		XCTAssertEqual(height, 624)
	}

	func testMeasuredOverflowCapsAtTheAvailableScreenHeight() {
		let height = AccountPanelLayout.accountListHeight(
			accountCount: 12,
			measuredContentHeight: 900,
			windowVisibleFrame: NSRect(x: 0, y: 0, width: 800, height: 560)
		)

		XCTAssertEqual(
			height,
			560
				- AccountPanelLayout.screenVerticalMargin
				- AccountPanelLayout.panelVerticalPadding
				- AccountPanelLayout.fixedChromeHeight
		)
	}

	func testBoundedStatusViewportReducesAccountViewportWithinScreen() {
		let height = AccountPanelLayout.accountListHeight(
			accountCount: 6,
			measuredContentHeight: 900,
			windowVisibleFrame: NSRect(x: 0, y: 0, width: 800, height: 675),
			additionalChromeHeight: AccountPanelLayout.statusMaximumHeight
		)

		XCTAssertEqual(
			height,
			675
				- AccountPanelLayout.screenVerticalMargin
				- AccountPanelLayout.panelVerticalPadding
				- AccountPanelLayout.fixedChromeHeight
				- AccountPanelLayout.statusMaximumHeight
		)
	}

	func testFrameKeepsTopEdgeAndCenterWhenContentShrinks() {
		let currentFrame = NSRect(x: 120, y: 200, width: 360, height: 700)
		let frame = PanelWindowSizingLayout.frame(
			forContentSize: NSSize(width: AccountPanelLayout.panelWidth, height: 520),
			currentFrame: currentFrame
		) { contentSize in
			NSSize(width: contentSize.width + 18, height: contentSize.height + 18)
		} visibleFrame: {
			NSRect(x: 0, y: 0, width: 1_000, height: 1_000)
		}

		XCTAssertEqual(frame.width, 294)
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
		let visibleFrame = NSRect(x: 0, y: 0, width: 800, height: 700)
		let frame = PanelWindowSizingLayout.frame(
			forContentSize: NSSize(width: 900, height: 760),
			currentFrame: NSRect(x: 100, y: 100, width: 340, height: 220)
		) { contentSize in
			contentSize
		} visibleFrame: {
			visibleFrame
		}

		XCTAssertEqual(frame.minX, 8)
		XCTAssertEqual(frame.minY, 8)
		XCTAssertEqual(frame.width, 784)
		XCTAssertEqual(frame.height, 684)
		XCTAssertEqual(frame.maxX, 792)
		XCTAssertEqual(frame.maxY, 692)
	}

	func testOversizedFittedFrameConvergesOnTheSecondLayoutPass() {
		let visibleFrame = NSRect(x: 0, y: 0, width: 800, height: 700)
		let firstFrame = PanelWindowSizingLayout.frame(
			forContentSize: NSSize(width: 900, height: 760),
			currentFrame: NSRect(x: 100, y: 100, width: 340, height: 220)
		) { $0 } visibleFrame: {
			visibleFrame
		}
		let secondFrame = PanelWindowSizingLayout.frame(
			forContentSize: NSSize(width: 900, height: 760),
			currentFrame: firstFrame
		) { $0 } visibleFrame: {
			visibleFrame
		}

		XCTAssertEqual(secondFrame, firstFrame)
	}
}
