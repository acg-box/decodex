import AppKit
import XCTest
@testable import DecodexApp

@MainActor
final class HoverHintTests: XCTestCase {
	private func ownerWindow() async throws -> (NSWindow, HoverHintAnchor) {
		_ = NSApplication.shared
		let window = TransparentStatusPanel(contentRect: NSRect(x: 300, y: 300, width: 240, height: 100), styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
		window.isReleasedWhenClosed = false
		let anchor = HoverHintAnchor(frame: NSRect(x: 20, y: 20, width: 30, height: 24))
		window.contentView?.addSubview(anchor)
		window.orderFrontRegardless()
		window.makeKey()
		try await Task.sleep(for: .milliseconds(100))
		return (window, anchor)
	}

	private var visibleHints: [NSWindow] {
		NSApp.windows.filter { $0.title == "Decodex Hint" && $0.isVisible }
	}

	func testHintWaitsThenSharesTheOverlayWithoutTakingInput() async throws {
		let (window, anchor) = try await ownerWindow()
		defer { HoverHint.shared.hide(owner: anchor); window.close() }
		HoverHint.shared.schedule(owner: anchor, text: "Copy (⌘C)")
		XCTAssertTrue(visibleHints.isEmpty)
		try await Task.sleep(for: .milliseconds(600))
		let hint = try XCTUnwrap(visibleHints.first)
		XCTAssertEqual(visibleHints.count, 1)
		XCTAssertTrue(hint.ignoresMouseEvents)
		XCTAssertFalse(hint.isKeyWindow)
		XCTAssertTrue(window.isKeyWindow)
		let trigger = window.convertToScreen(anchor.convert(anchor.bounds, to: nil))
		XCTAssertEqual(hint.frame.maxY, trigger.minY - 8, accuracy: 1)
		XCTAssertEqual(hint.frame.minX, trigger.minX, accuracy: 1)
		XCTAssertFalse(hint.frame.intersects(trigger))
		XCTAssertTrue(hint.contentView is NSGlassEffectView || hint.contentView is ThinMaterialView)
		XCTAssertLessThanOrEqual(hint.frame.width, 360)
		HoverHint.shared.hide(owner: anchor)
		XCTAssertTrue(visibleHints.isEmpty)
	}

	func testLeavingAndRemovingTheOwnerCancelPendingHints() async throws {
		let (window, anchor) = try await ownerWindow()
		defer { HoverHint.shared.hide(owner: anchor); window.close() }
		HoverHint.shared.schedule(owner: anchor, text: "An obsolete hint must not appear.")
		HoverHint.shared.hide(owner: anchor)
		try await Task.sleep(for: .milliseconds(600))
		XCTAssertTrue(visibleHints.isEmpty)
		HoverHint.shared.schedule(owner: anchor, text: "This owner will be removed.")
		anchor.removeFromSuperview()
		try await Task.sleep(for: .milliseconds(600))
		XCTAssertTrue(visibleHints.isEmpty)
	}

	func testMovingToAnotherOwnerReplacesThePendingHint() async throws {
		let (window, first) = try await ownerWindow()
		let second = HoverHintAnchor(frame: NSRect(x: 90, y: 20, width: 30, height: 24))
		window.contentView?.addSubview(second)
		defer { HoverHint.shared.hide(owner: second); window.close() }
		HoverHint.shared.schedule(owner: first, text: "First")
		HoverHint.shared.schedule(owner: second, text: "Second")
		HoverHint.shared.hide(owner: first)
		try await Task.sleep(for: .milliseconds(600))
		let hint = try XCTUnwrap(visibleHints.first)
		func labels(_ view: NSView) -> [String] {
			if let label = view as? NSTextField { return [label.stringValue] }
			return view.subviews.flatMap(labels)
		}
		XCTAssertEqual(labels(try XCTUnwrap(hint.contentView)), ["Second"])
		XCTAssertEqual(visibleHints.count, 1)
	}

	func testWrappedPathUsesItsContentWidthWithoutAddingLines() async throws {
		let (window, anchor) = try await ownerWindow()
		defer { HoverHint.shared.hide(owner: anchor); window.close() }
		HoverHint.shared.schedule(owner: anchor, text: "/Users/x/.codex/diagnostics/decodex-history-english-qa-20261014/English-QA")
		try await Task.sleep(for: .milliseconds(600))
		let hint = try XCTUnwrap(visibleHints.first)
		XCTAssertLessThan(hint.frame.width, 320)
		XCTAssertLessThanOrEqual(hint.frame.height, 50)
	}
}
