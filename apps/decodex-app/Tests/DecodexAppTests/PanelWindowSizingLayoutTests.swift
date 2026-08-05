import AppKit
import SwiftUI
@testable import DecodexApp
import XCTest

final class PanelWindowSizingLayoutTests: XCTestCase {
	func testPanelSpacingUsesOneCompactTwoPointRhythm() {
		XCTAssertEqual(PanelSpacing.micro, 2)
		XCTAssertEqual(PanelSpacing.compact, 4)
		XCTAssertEqual(PanelSpacing.related, 6)
		XCTAssertEqual(PanelSpacing.section, 8)
		XCTAssertEqual(PanelSpacing.cardHorizontal, 10)
		XCTAssertEqual(PanelSpacing.cardVertical, 8)
		XCTAssertEqual(PanelSpacing.popoverInset, 12)
	}

	func testAccountReorderMovesOnlyAfterCrossingAnAdjacentCardCenter() {
		let order = ["first", "second", "third"]
		let frames = accountReorderFrames()

		XCTAssertEqual(
			AccountCardReorderLayout.reorderedAccountIDs(
				dragging: "first",
				baseOrder: order,
				frames: frames,
				translationY: 87
			),
			order
		)
		XCTAssertEqual(
			AccountCardReorderLayout.reorderedAccountIDs(
				dragging: "first",
				baseOrder: order,
				frames: frames,
				translationY: 88
			),
			["second", "first", "third"]
		)
	}

	func testAccountReorderConstrainKeepsTheWholeCardInItsVerticalTrack() {
		let order = ["first", "second", "third"]
		let frames = accountReorderFrames()

		XCTAssertEqual(
			AccountCardReorderLayout.constrainedTranslationY(
				for: "first",
				baseOrder: order,
				frames: frames,
				proposed: 500
			),
			176
		)
		XCTAssertEqual(
			AccountCardReorderLayout.constrainedTranslationY(
				for: "third",
				baseOrder: order,
				frames: frames,
				proposed: -500
			),
			-176
		)
	}

	func testAccountReorderOffsetsOpenTheSelectedSlot() {
		let order = ["first", "second", "third"]
		let visualOrder = ["second", "first", "third"]
		let frames = accountReorderFrames()

		XCTAssertEqual(
			AccountCardReorderLayout.verticalOffset(
				for: "second",
				baseOrder: order,
				visualOrder: visualOrder,
				frames: frames,
				spacing: PanelSpacing.section
			),
			-88
		)
		XCTAssertEqual(
			AccountCardReorderLayout.verticalOffset(
				for: "first",
				baseOrder: order,
				visualOrder: visualOrder,
				frames: frames,
				spacing: PanelSpacing.section
			),
			88
		)
	}

	func testAccountReorderRebasesFramesBeforeASecondDrag() throws {
		let initialOrder = ["first", "second", "third"]
		let reordered = ["second", "first", "third"]
		let rebasedFrames = try XCTUnwrap(
			AccountCardReorderLayout.rebasedFrames(
				from: initialOrder,
				to: reordered,
				frames: accountReorderFrames(),
				spacing: PanelSpacing.section
			)
		)

		XCTAssertEqual(rebasedFrames["second"]?.minY, 0)
		XCTAssertEqual(rebasedFrames["first"]?.minY, 88)
		XCTAssertEqual(rebasedFrames["third"]?.minY, 176)
		XCTAssertEqual(
			AccountCardReorderLayout.constrainedTranslationY(
				for: "third",
				baseOrder: reordered,
				frames: rebasedFrames,
				proposed: -500
			),
			-176
		)
		XCTAssertEqual(
			AccountCardReorderLayout.reorderedAccountIDs(
				dragging: "third",
				baseOrder: reordered,
				frames: rebasedFrames,
				translationY: -176
			),
			["third", "second", "first"]
		)
	}

	private func accountReorderFrames() -> [String: CGRect] {
		[
			"first": CGRect(x: 0, y: 0, width: 260, height: 80),
			"second": CGRect(x: 0, y: 88, width: 260, height: 80),
			"third": CGRect(x: 0, y: 176, width: 260, height: 80),
		]
	}

	@MainActor
	func testPanelWindowHostStaysTransparentWithoutOverridingSystemAppearance() {
		let window = NSWindow(
			contentRect: NSRect(x: 0, y: 0, width: 320, height: 480),
			styleMask: [.borderless],
			backing: .buffered,
			defer: false
		)
		window.hasShadow = true
		window.isOpaque = true
		window.backgroundColor = .windowBackgroundColor

		PanelWindowAppearance.apply(to: window)

		XCTAssertFalse(window.hasShadow)
		XCTAssertFalse(window.isOpaque)
		XCTAssertEqual(window.backgroundColor, .clear)
		XCTAssertEqual(window.contentView?.layer?.backgroundColor?.alpha, 0)
		XCTAssertNil(window.appearance)
	}

	@MainActor
	func testStatusPanelHostingViewDoesNotPaintAWindowSizedBackdrop() {
		let hostingView = TransparentHostingView(rootView: Text("Decodex"))

		XCTAssertFalse(hostingView.isOpaque)
	}

	@MainActor
	func testPanelWindowAppearanceIgnoresDetachedHost() {
		PanelWindowAppearance.apply(to: nil)
	}

	@MainActor
	func testCustomStatusPanelIsBorderlessTransparentAndKeyCapable() {
		let panel = TransparentStatusPanel(
			contentRect: NSRect(x: 0, y: 0, width: 320, height: 480),
			styleMask: [.borderless],
			backing: .buffered,
			defer: false
		)

		PanelWindowAppearance.apply(to: panel)

		XCTAssertEqual(panel.styleMask, [.borderless])
		XCTAssertFalse(panel.isOpaque)
		XCTAssertEqual(panel.backgroundColor, .clear)
		XCTAssertFalse(panel.hasShadow)
		XCTAssertTrue(panel.canBecomeKey)
		XCTAssertFalse(panel.canBecomeMain)
	}

	func testStatusPanelOriginUsesTheStatusItemsDisplayAndKeepsThePanelVisible() {
		let origin = StatusPanelLayout.origin(
			anchorRect: NSRect(x: 2_470, y: 1_320, width: 30, height: 24),
			panelSize: NSSize(width: 340, height: 620),
			visibleFrame: NSRect(x: 1_920, y: 0, width: 1_080, height: 1_320)
		)

		XCTAssertEqual(origin, NSPoint(x: 2_315, y: 692))
	}

	func testStatusPanelSelectsTheDisplayContainingTheStatusItemBeforeAStaleFallback() {
		let screenFrames = [
			NSRect(x: 0, y: 0, width: 1_920, height: 1_080),
			NSRect(x: 1_920, y: 0, width: 1_080, height: 1_344),
		]

		XCTAssertEqual(
			StatusPanelLayout.screenIndex(
				containing: NSRect(x: 2_470, y: 1_320, width: 30, height: 24),
				screenFrames: screenFrames,
				fallbackIndex: 0
			),
			1
		)
	}

	func testStatusPanelOriginClampsAtBothHorizontalDisplayEdges() {
		let visibleFrame = NSRect(x: 1_000, y: 100, width: 800, height: 600)
		let panelSize = NSSize(width: 340, height: 400)

		XCTAssertEqual(
			StatusPanelLayout.origin(
				anchorRect: NSRect(x: 1_000, y: 680, width: 24, height: 20),
				panelSize: panelSize,
				visibleFrame: visibleFrame
			).x,
			1_008
		)
		XCTAssertEqual(
			StatusPanelLayout.origin(
				anchorRect: NSRect(x: 1_776, y: 680, width: 24, height: 20),
				panelSize: panelSize,
				visibleFrame: visibleFrame
			).x,
			1_452
		)
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

	func testAccountListEstimateUsesRowHeightAndSharedSectionSpacing() {
		XCTAssertEqual(
			AccountPanelLayout.estimatedAccountListContentHeight(
				accountCount: 6
			),
			546
		)
		XCTAssertEqual(
			AccountPanelLayout.estimatedAccountListContentHeight(
				accountCount: 0
			),
			86
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
