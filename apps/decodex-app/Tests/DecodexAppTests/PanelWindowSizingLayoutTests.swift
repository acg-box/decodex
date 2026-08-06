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

	func testStatusPanelCentersBelowTheStatusItemWithoutUsingScreenCornerPlacement() {
		let origin = StatusPanelLayout.origin(
			anchorRect: NSRect(x: 1_000, y: 1_000, width: 24, height: 24),
			panelSize: NSSize(width: 320, height: 480),
			visibleFrame: NSRect(x: 0, y: 0, width: 1_920, height: 1_080),
			screenMargin: 8,
			menuBarGap: 4
		)

		XCTAssertEqual(origin.x, 852)
		XCTAssertEqual(origin.y, 516)
	}

	func testStatusPanelOriginClampsOnlyWhenTheAnchorIsNearAScreenEdge() {
		let origin = StatusPanelLayout.origin(
			anchorRect: NSRect(x: 1_900, y: 1_000, width: 20, height: 24),
			panelSize: NSSize(width: 320, height: 480),
			visibleFrame: NSRect(x: 0, y: 0, width: 1_920, height: 1_080),
			screenMargin: 8,
			menuBarGap: 4
		)

		XCTAssertEqual(origin.x, 1_592)
		XCTAssertEqual(origin.y, 516)
	}

	@MainActor
	func testStatusPanelUsesATransparentBorderlessWindow() {
		let panel = TransparentStatusPanel(
			contentRect: .zero,
			styleMask: [.borderless],
			backing: .buffered,
			defer: true
		)

		PanelWindowAppearance.apply(to: panel)

		XCTAssertTrue(panel.styleMask.contains(.borderless))
		XCTAssertFalse(panel.isOpaque)
		XCTAssertEqual(panel.backgroundColor, .clear)
		XCTAssertFalse(panel.hasShadow)
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

}
