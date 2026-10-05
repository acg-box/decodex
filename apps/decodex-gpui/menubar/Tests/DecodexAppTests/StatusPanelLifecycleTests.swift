import AppKit
import XCTest
import SwiftUI
import QuartzCore
@testable import DecodexApp

@MainActor
final class StatusPanelLifecycleTests: XCTestCase {
	func testLoginRefreshRowDoesNotReserveQuotaOrFooterSpace() throws {
		let authority = ResetCardAuthority(profileName: "local", serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")
		let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
		defer { try? FileManager.default.removeItem(at: root) }
		let store = ResetCardStore(client: EmptyWidgetClient(), pendingStore: ResetCardPendingAttemptStore(nativeRequest: NativeJournalFixture.request, journalURL: root.appendingPathComponent("pending.json")))
		func render(loginRequired: Bool, hasQuota: Bool) throws -> NSImage {
			let quota = ResetCardQuotaWindow(durationMinutes: 300, observedAtUnixMicros: nil, state: hasQuota ? .current(usedPercent: 42, resetsAtUnixMicros: 2_000_000_000_000_000) : .unknown)
			let account = ResetCardAccountRecord(authority: authority, accountID: "11111111-1111-4111-8111-111111111111", alias: "Account TEST0-00000", accountRevision: 1, enabled: true, observedState: loginRequired ? .authFailed : .available, lifecycleReadiness: .ready, credentialBinding: AccountCredentialBinding(schemaVersion: 1, version: 1, fingerprintSHA256: String(repeating: "a", count: 64), provider: .chatGPT, providerAccountID: "fixture-account"), fiveHourQuota: quota, sevenDayQuota: .unknown(durationMinutes: 10_080))
			let state = ResetCardAccountState(account: account, inventory: nil, error: nil, isRefreshing: false)
			let renderer = ImageRenderer(content: ResetCardAccountRow(state: state, store: store).frame(width: 320).background(Color(nsColor: .windowBackgroundColor)).environment(\.colorScheme, .dark))
			return try XCTUnwrap(renderer.nsImage)
		}
		let login = try render(loginRequired: true, hasQuota: false)
		let loginWithQuota = try render(loginRequired: true, hasQuota: true)
		let empty = try render(loginRequired: false, hasQuota: false)
		let available = try render(loginRequired: false, hasQuota: true)
		XCTAssertEqual(login.size.height, loginWithQuota.size.height, "Cached usage must not add rows during login recovery")
		XCTAssertEqual(empty.size.height, 20 + 2 * PanelSpacing.cardVertical, accuracy: 1, "An empty quota container and invisible reorder handle must not reserve rows")
		XCTAssertEqual(login.size.height, empty.size.height, "Login recovery must stay in the header without a status row")
		XCTAssertGreaterThan(available.size.height, empty.size.height, "Healthy accounts must retain their usage rows")
	}

	func testAccountDisclosuresRemainIndependent() throws {
		let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
		defer { try? FileManager.default.removeItem(at: root) }
		let store = ResetCardStore(client: EmptyWidgetClient(), pendingStore: ResetCardPendingAttemptStore(nativeRequest: NativeJournalFixture.request, journalURL: root.appendingPathComponent("pending.json")))
		var expanded: Set<String> = []
		let binding = Binding(get: { expanded }, set: { expanded = $0 })
		func row(_ id: String) -> ResetCardAccountRow {
			let account = ResetCardAccountRecord(authority: ResetCardAuthority(profileName: "local", serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"), accountID: id, alias: id, accountRevision: 1, enabled: true, observedState: .available, lifecycleReadiness: .ready, fiveHourQuota: .unknown(durationMinutes: 300), sevenDayQuota: .unknown(durationMinutes: 10_080))
			return ResetCardAccountRow(state: ResetCardAccountState(account: account, inventory: nil, error: nil, isRefreshing: false), store: store, detailedAccountIDs: binding)
		}
		let first = row("11111111-1111-4111-8111-111111111111")
		let second = row("22222222-2222-4222-8222-222222222222")
		first.detailsBinding.wrappedValue = true
		second.detailsBinding.wrappedValue = true
		XCTAssertTrue(first.detailsBinding.wrappedValue)
		XCTAssertTrue(second.detailsBinding.wrappedValue)
		first.detailsBinding.wrappedValue = false
		XCTAssertFalse(first.detailsBinding.wrappedValue)
		XCTAssertTrue(second.detailsBinding.wrappedValue)
	}

	func testAccountPaddingClicksToggleButActionClicksDoNot() async throws {
		let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
		defer { try? FileManager.default.removeItem(at: root) }
		let store = ResetCardStore(client: EmptyWidgetClient(), pendingStore: ResetCardPendingAttemptStore(nativeRequest: NativeJournalFixture.request, journalURL: root.appendingPathComponent("pending.json")))
		let account = ResetCardAccountRecord(authority: ResetCardAuthority(profileName: "local", serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"), accountID: "11111111-1111-4111-8111-111111111111", alias: "Test account", accountRevision: 1, enabled: true, observedState: .available, lifecycleReadiness: .ready, fiveHourQuota: .unknown(durationMinutes: 300), sevenDayQuota: .unknown(durationMinutes: 10_080))
		var expanded: Set<String> = []
		let host = NSHostingView(rootView: ResetCardAccountRow(state: ResetCardAccountState(account: account, inventory: nil, error: nil, isRefreshing: false), store: store, detailedAccountIDs: Binding(get: { expanded }, set: { expanded = $0 })).frame(width: 320))
		let window = NSPanel(contentRect: NSRect(x: 100, y: 100, width: 320, height: 36), styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
		window.contentView = host
		window.orderFrontRegardless()
		defer { window.orderOut(nil) }
		try await Task.sleep(for: .milliseconds(100))
		func click(_ point: CGPoint) throws {
			let location = host.convert(point, to: nil)
			let down = try XCTUnwrap(NSEvent.mouseEvent(with: .leftMouseDown, location: location, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil, eventNumber: 1, clickCount: 1, pressure: 1))
			let up = try XCTUnwrap(NSEvent.mouseEvent(with: .leftMouseUp, location: location, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime + 0.02, windowNumber: window.windowNumber, context: nil, eventNumber: 2, clickCount: 1, pressure: 0))
			window.sendEvent(down)
			window.sendEvent(up)
		}
		try click(CGPoint(x: 3, y: 3))
		try await Task.sleep(for: .milliseconds(50))
		XCTAssertTrue(expanded.contains(account.accountID), "Card padding must toggle details")
		try click(CGPoint(x: 100, y: 18))
		try await Task.sleep(for: .milliseconds(50))
		XCTAssertTrue(expanded.isEmpty, "Identity click must toggle exactly once")
		try click(CGPoint(x: 300, y: 18))
		try await Task.sleep(for: .milliseconds(50))
		XCTAssertTrue(expanded.isEmpty, "An account action must not toggle details")
	}

	func testPanelSizeTracksLayoutWithoutASecondAnimation() async throws {
		let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
		defer { try? FileManager.default.removeItem(at: root) }
		let store = ResetCardStore(client: EmptyWidgetClient(), pendingStore: ResetCardPendingAttemptStore(nativeRequest: NativeJournalFixture.request, journalURL: root.appendingPathComponent("pending.json")))
		let controller = StatusPanelController(store: store)
		defer { controller.invalidate() }
		controller.togglePanel()
		try await Task.sleep(for: .milliseconds(350))
		let initial = controller.panel.frame.size
		let top = controller.panel.frame.maxY
		let onFrameChange = controller.panel.onFrameChange
		var frames: [CGRect] = []
		controller.panel.onFrameChange = {
			frames.append(controller.panel.frame)
			onFrameChange?()
		}
		defer { controller.panel.onFrameChange = onFrameChange }
		controller.updatePanelContentSize(CGSize(width: initial.width, height: initial.height + 100))
		XCTAssertEqual(controller.panel.frame.height, initial.height + 100, accuracy: 1, "The window must match the current animated layout immediately")
		controller.updatePanelContentSize(initial)
		try await Task.sleep(for: .milliseconds(360))
		XCTAssertEqual(controller.panel.frame.height, initial.height, accuracy: 1)
		XCTAssertGreaterThanOrEqual(frames.count, 2)
		for frame in frames { XCTAssertEqual(frame.maxY, top, accuracy: 0.5, "Every committed frame must preserve the top edge") }
	}

	func testMultipleCardsHaveScrollableOverflowWithoutScrollbars() async throws {
		let authority = ResetCardAuthority(profileName: "local", serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")
		let account = ResetCardAccountRecord(authority: authority, accountID: "11111111-1111-4111-8111-111111111111", alias: "Test", accountRevision: 1, enabled: true, observedState: .available, lifecycleReadiness: .ready, fiveHourQuota: .unknown(durationMinutes: 300), sevenDayQuota: .unknown(durationMinutes: 10_080))
		let cards = try (1...5).map { try ResetCardDescriptor(grantedAtUnixSeconds: Int64($0), expiresAtUnixSeconds: 2_000_000_000 + Int64($0 * 100)) }
		let inventory = ResetCardInventory(authority: authority, accountID: account.accountID, accountRevision: 1, cards: cards, fiveHourQuota: .unknown(durationMinutes: 300), sevenDayQuota: .unknown(durationMinutes: 10_080), observationError: nil)
		let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
		defer { try? FileManager.default.removeItem(at: root) }
		let store = ResetCardStore(client: EmptyWidgetClient(), pendingStore: ResetCardPendingAttemptStore(nativeRequest: NativeJournalFixture.request, journalURL: root.appendingPathComponent("pending.json")))
		let state = ResetCardAccountState(account: account, inventory: inventory, error: nil, isRefreshing: false)
		let host = NSHostingView(rootView: ResetCardAccountRow(state: state, store: store, detailedAccountIDs: .constant([account.accountID])))
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
		let next = try XCTUnwrap(buttons(host).first { $0.toolTip == "Next reset cards" })
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

	func testReopeningPanelRefreshesExternallyChangedFastMode() async throws {
		let client = MutablePanelFastModeClient()
		let fastMode = FastModeStore(client: client)
		let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
		defer { try? FileManager.default.removeItem(at: root) }
		let store = ResetCardStore(
			client: EmptyWidgetClient(),
			pendingStore: ResetCardPendingAttemptStore(nativeRequest: NativeJournalFixture.request, journalURL: root.appendingPathComponent("pending.json"))
		)
		let controller = StatusPanelController(store: store, fastModeStore: fastMode)
		defer { controller.invalidate() }
		controller.togglePanel()
		for _ in 0..<100 {
			if fastMode.isEnabled { break }
			try await Task.sleep(for: .milliseconds(5))
		}
		XCTAssertTrue(fastMode.isEnabled)
		controller.togglePanel()
		await client.changeExternally(to: false)
		controller.togglePanel()
		for _ in 0..<100 {
			if !fastMode.isEnabled { break }
			try await Task.sleep(for: .milliseconds(5))
		}
		XCTAssertFalse(fastMode.isEnabled, "Reopening must refresh the global Fast setting.")
		let writes = await client.writes
		XCTAssertEqual(writes, 0, "Showing the panel must only read the setting.")
	}

	func testWidgetOpensWithoutActivationAndSurvivesFocusChanges() async throws {
		let application = NSApplication.shared
		let foregroundPID = NSWorkspace.shared.frontmostApplication?.processIdentifier
		let workspace = NSWindow(contentRect: NSRect(x: 100, y: 100, width: 300, height: 200), styleMask: [.titled], backing: .buffered, defer: false)
		workspace.isReleasedWhenClosed = false
		defer { workspace.close() }
		let policy = application.activationPolicy()
		let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
		defer { try? FileManager.default.removeItem(at: root) }
		let store = ResetCardStore(
			client: EmptyWidgetClient(),
			pendingStore: ResetCardPendingAttemptStore(nativeRequest: NativeJournalFixture.request, journalURL: root.appendingPathComponent("pending.json"))
		)
		let controller = StatusPanelController(store: store, fastModeStore: FastModeStore(client: MutablePanelFastModeClient()))
		defer { controller.invalidate() }
		controller.togglePanel()
		try await Task.sleep(for: .milliseconds(100))
		XCTAssertTrue(controller.panel.isVisible)
		XCTAssertGreaterThan(controller.panel.frame.width, 0)
		XCTAssertGreaterThan(controller.panel.frame.height, 0)
		XCTAssertTrue(controller.panel.styleMask.contains(.nonactivatingPanel))
		XCTAssertTrue(controller.panel.isKeyWindow, "The initial status-item click must focus the menu")
		XCTAssertFalse(controller.panel.isMainWindow)
		XCTAssertFalse(workspace.isVisible, "Opening the menu must not reveal the main workspace")
		// AppKit may report local activation while a nonactivating panel owns
		// key focus. The foreground application and workspace visibility must stay unchanged.
		XCTAssertEqual(NSWorkspace.shared.frontmostApplication?.processIdentifier, foregroundPID)
		XCTAssertEqual(application.activationPolicy(), policy)
		NotificationCenter.default.post(name: NSWindow.didResignKeyNotification, object: controller.panel)
		XCTAssertTrue(controller.panel.isVisible, "Opening a child control must not dismiss the widget")
		controller.togglePanel()
		XCTAssertFalse(controller.panel.isVisible)
		controller.togglePanel()
		XCTAssertTrue(controller.panel.isVisible)
		XCTAssertTrue(controller.panel.isKeyWindow, "Reopening must also focus the menu")
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

private actor MutablePanelFastModeClient: FastModeClient {
	private var enabled = true
	private(set) var writes = 0

	func status() async throws -> Bool { enabled }
	func setEnabled(_ enabled: Bool) async throws -> Bool {
		writes += 1
		self.enabled = enabled
		return enabled
	}
	func changeExternally(to enabled: Bool) { self.enabled = enabled }
}
