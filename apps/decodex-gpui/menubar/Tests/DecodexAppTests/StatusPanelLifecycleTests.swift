import AppKit
import XCTest
import SwiftUI
import QuartzCore
@testable import DecodexApp

@MainActor
final class StatusPanelLifecycleTests: XCTestCase {
	func testMultipleCardsHaveScrollableOverflowWithoutScrollbars() async throws {
		let authority = ResetCardAuthority(profileName: "local", serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")
		let account = ResetCardAccountRecord(authority: authority, accountID: "11111111-1111-4111-8111-111111111111", alias: "Test", accountRevision: 1, enabled: true, observedState: .available, lifecycleReadiness: .ready, fiveHourQuota: .unknown(durationMinutes: 300), sevenDayQuota: .unknown(durationMinutes: 10_080))
		let cards = try (1...5).map { try ResetCardDescriptor(grantedAtUnixSeconds: Int64($0), expiresAtUnixSeconds: 2_000_000_000 + Int64($0 * 100)) }
		let inventory = ResetCardInventory(authority: authority, accountID: account.accountID, accountRevision: 1, cards: cards, fiveHourQuota: .unknown(durationMinutes: 300), sevenDayQuota: .unknown(durationMinutes: 10_080), observationError: nil)
		let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
		defer { try? FileManager.default.removeItem(at: root) }
		let store = ResetCardStore(client: EmptyWidgetClient(), pendingStore: ResetCardPendingAttemptStore(journalURL: root.appendingPathComponent("pending.json")))
		let state = ResetCardAccountState(account: account, inventory: inventory, error: nil, isRefreshing: false)
		let host = NSHostingView(rootView: ResetCardAccountRow(state: state, store: store))
		let window = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 300, height: 120), styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
		window.contentView = host
		window.orderFrontRegardless()
		defer { window.orderOut(nil) }
		try await Task.sleep(for: .milliseconds(100))
		host.layoutSubtreeIfNeeded()
		func scrollViews(_ view: NSView) -> [NSScrollView] {
			(view as? NSScrollView).map { [$0] } ?? view.subviews.flatMap(scrollViews)
		}
		let scroll = try XCTUnwrap(scrollViews(host).first)
		let document = try XCTUnwrap(scroll.documentView)
		XCTAssertGreaterThan(document.frame.width, scroll.contentView.bounds.width)
		XCTAssertFalse(scroll.hasHorizontalScroller)
		XCTAssertFalse(scroll.hasVerticalScroller)
		let wheelScroll = try XCTUnwrap(scroll as? CardWheelScrollView)
		let wheel = try XCTUnwrap(CGEvent(scrollWheelEvent2Source: nil, units: .line, wheelCount: 1, wheel1: -3, wheel2: 0, wheel3: 0))
		wheelScroll.reduceMotion = { false }
		wheelScroll.scrollWheel(with: try XCTUnwrap(NSEvent(cgEvent: wheel)))
		try await Task.sleep(for: .milliseconds(300))
		XCTAssertGreaterThan(scroll.contentView.bounds.minX, 0, "An ordinary vertical wheel must move the card strip")
		wheelScroll.move(by: -10_000)
		func buttons(_ view: NSView) -> [NSButton] {
			(view as? NSButton).map { [$0] } ?? view.subviews.flatMap(buttons)
		}
		let next = try XCTUnwrap(buttons(host).first { $0.toolTip == "Next Reset Cards" })
		XCTAssertFalse(next.isHidden)
		next.performClick(nil)
		try await Task.sleep(for: .milliseconds(70))
		XCTAssertGreaterThan(scroll.contentView.bounds.minX, 0, "The visible next button must move the strip")
		for _ in 0..<10 { next.performClick(nil) }
		try await Task.sleep(for: .milliseconds(300))
		XCTAssertEqual(scroll.contentView.bounds.maxX, document.frame.width, accuracy: 1)
		XCTAssertFalse(next.isEnabled)
		wheelScroll.move(by: -80, animated: true)
		wheelScroll.move(by: -10_000)
		try await Task.sleep(for: .milliseconds(300))
		XCTAssertEqual(scroll.contentView.bounds.minX, 0, accuracy: 1, "Direct input must cancel a pending animation")

	}

	func testSingleCardStaysLeadingAndReducedMotionIsImmediate() {
		let view = CardScrollerView(content: Text("One card").fixedSize())
		view.frame = NSRect(x: 0, y: 0, width: 300, height: 22)
		view.layoutSubtreeIfNeeded()
		XCTAssertLessThan(view.host.frame.width, view.scroll.contentView.bounds.width)
		XCTAssertEqual(view.host.frame.minX, 0)
		XCTAssertTrue(view.previous.isHidden)
		XCTAssertTrue(view.next.isHidden)
		view.host.frame.size.width = 600
		view.scroll.reduceMotion = { true }
		view.scroll.move(by: 100, animated: true)
		XCTAssertEqual(view.scroll.contentView.bounds.minX, 100, accuracy: 1)
	}

	func testArrowUsesCompositorWhileMainThreadIsBusy() async throws {
		let (view, panel) = try await animatedStrip()
		defer { panel.orderOut(nil) }
		let point = view.next.convert(NSPoint(x: 8, y: 11), to: nil)
		let down = try XCTUnwrap(NSEvent.mouseEvent(with: .leftMouseDown, location: point, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: panel.windowNumber, context: nil, eventNumber: 1, clickCount: 1, pressure: 1))
		let up = try XCTUnwrap(NSEvent.mouseEvent(with: .leftMouseUp, location: point, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: panel.windowNumber, context: nil, eventNumber: 2, clickCount: 1, pressure: 0))
		NSApp.postEvent(up, atStart: true)
		view.next.mouseDown(with: down)
		let target = view.scroll.contentView.bounds.width * 0.8
		let animation = try XCTUnwrap(view.scroll.contentView.layer?.animation(forKey: CardWheelScrollView.animationKey) as? CABasicAnimation)
		XCTAssertEqual(animation.fromValue as? CGFloat, 0)
		XCTAssertEqual(animation.toValue as? CGFloat, target)
		XCTAssertEqual(animation.preferredFrameRateRange.preferred, Float(try XCTUnwrap(panel.screen).maximumFramesPerSecond))
		XCTAssertEqual(view.scroll.contentView.bounds.minX, target, accuracy: 0.01)
		CATransaction.flush()
		blockMainThreadForTest()
		let displayed = view.scroll.visibleOffset
		XCTAssertGreaterThan(displayed, 0)
		XCTAssertLessThan(displayed, target)
		XCTAssertEqual(view.scroll.contentView.bounds.minX, target, accuracy: 0.01)
		try await Task.sleep(for: .milliseconds(300))
		XCTAssertFalse(view.scroll.isAnimating)
		XCTAssertEqual(view.scroll.visibleOffset, target, accuracy: 1)
	}

	func testInterruptAndRetargetUseDisplayedPosition() async throws {
		let (view, panel) = try await animatedStrip()
		defer { panel.orderOut(nil) }
		view.scroll.move(by: 200, animated: true)
		try await Task.sleep(for: .milliseconds(70))
		let before = view.scroll.visibleOffset
		view.scroll.move(by: 100, animated: true)
		let animation = try XCTUnwrap(view.scroll.contentView.layer?.animation(forKey: CardWheelScrollView.animationKey) as? CABasicAnimation)
		XCTAssertEqual(try XCTUnwrap(animation.fromValue as? CGFloat), before, accuracy: 4)
		XCTAssertEqual(view.scroll.contentView.bounds.minX, 300, accuracy: 1)
		var controlPoint = [Float](repeating: 0, count: 2)
		animation.timingFunction?.getControlPoint(at: 1, values: &controlPoint)
		XCTAssertGreaterThan(controlPoint[1], 0, "Retargeting must preserve forward velocity")
		try await Task.sleep(for: .milliseconds(70))
		let interrupted = view.scroll.visibleOffset
		view.scroll.move(by: -10)
		XCTAssertFalse(view.scroll.isAnimating)
		XCTAssertEqual(view.scroll.visibleOffset, interrupted - 10, accuracy: 4)
		let stopped = view.scroll.visibleOffset
		try await Task.sleep(for: .milliseconds(300))
		XCTAssertEqual(view.scroll.visibleOffset, stopped, accuracy: 0.01)
	}

	func testMovingCardsCannotReceiveClicksAtDestinationCoordinates() async throws {
		let (view, panel) = try await animatedStrip()
		defer { panel.orderOut(nil) }
		view.scroll.move(by: 200, animated: true)
		XCTAssertTrue(view.host.hitTest(NSPoint(x: 20, y: 10)) === view.scroll)
		try await Task.sleep(for: .milliseconds(60))
		let visible = view.scroll.visibleOffset
		let down = try XCTUnwrap(NSEvent.mouseEvent(with: .leftMouseDown, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: panel.windowNumber, context: nil, eventNumber: 1, clickCount: 1, pressure: 1))
		view.scroll.mouseDown(with: down)
		XCTAssertFalse(view.scroll.isAnimating)
		XCTAssertEqual(view.scroll.visibleOffset, visible, accuracy: 4)
	}

	private func animatedStrip() async throws -> (CardScrollerView<AnyView>, NSPanel) {
		let content = AnyView(HStack { ForEach(0..<40) { Text("Card \($0)").padding(6) } }.fixedSize())
		let view = CardScrollerView(content: content)
		let panel = NSPanel(contentRect: NSRect(x: 100, y: 100, width: 300, height: 22), styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
		panel.contentView = view
		panel.orderFrontRegardless()
		try await Task.sleep(for: .milliseconds(100))
		view.layoutSubtreeIfNeeded()
		view.scroll.reduceMotion = { false }
		return (view, panel)
	}

	private func blockMainThreadForTest() {
		Thread.sleep(forTimeInterval: 0.08)
	}

	func testWidgetOpensWithoutActivationAndSurvivesFocusChanges() async throws {
		let application = NSApplication.shared
		let wasActive = application.isActive
		let policy = application.activationPolicy()
		let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
		defer { try? FileManager.default.removeItem(at: root) }
		let store = ResetCardStore(
			client: EmptyWidgetClient(),
			pendingStore: ResetCardPendingAttemptStore(journalURL: root.appendingPathComponent("pending.json"))
		)
		let controller = StatusPanelController(store: store)
		defer { controller.invalidate() }
		controller.togglePanel()
		try await Task.sleep(for: .milliseconds(100))
		XCTAssertTrue(controller.panel.isVisible)
		XCTAssertGreaterThan(controller.panel.frame.width, 0)
		XCTAssertGreaterThan(controller.panel.frame.height, 0)
		XCTAssertTrue(controller.panel.styleMask.contains(.nonactivatingPanel))
		XCTAssertEqual(application.isActive, wasActive)
		XCTAssertEqual(application.activationPolicy(), policy)
		NotificationCenter.default.post(name: NSWindow.didResignKeyNotification, object: controller.panel)
		XCTAssertTrue(controller.panel.isVisible, "Opening a child control must not dismiss the widget")
		controller.togglePanel()
		XCTAssertFalse(controller.panel.isVisible)
		controller.togglePanel()
		XCTAssertTrue(controller.panel.isVisible)
		controller.invalidate()
		XCTAssertFalse(controller.panel.isVisible)
	}
}

private actor EmptyWidgetClient: ResetCardClient {
	func accounts(authority _: ResetCardAuthority?) async throws -> [ResetCardAccountRecord] { [] }
	func inventory(for _: ResetCardAccountRecord) async throws -> ResetCardInventory { throw ResetCardClientError.invalidResponse }
	func use(_: ResetCardUseAttempt) async throws -> ResetCardOperationState { throw ResetCardClientError.invalidResponse }
	func status(for _: ResetCardUseAttempt) async throws -> ResetCardOperationState { throw ResetCardClientError.invalidResponse }
}
