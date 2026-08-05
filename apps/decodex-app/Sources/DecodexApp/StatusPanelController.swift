import AppKit
import SwiftUI

@MainActor
final class StatusPanelController: NSObject {
	private static let screenMargin: CGFloat = 8
	private static let menuBarGap: CGFloat = 4

	private let statusItem: NSStatusItem
	private let panel: TransparentStatusPanel
	private let hostingView: TransparentHostingView<StatusPanelRootView>
	private let store: ResetCardStore

	init(store: ResetCardStore) {
		self.store = store
		statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
		panel = TransparentStatusPanel(
			contentRect: .zero,
			styleMask: [.borderless],
			backing: .buffered,
			defer: true
		)
		hostingView = TransparentHostingView(rootView: StatusPanelRootView(store: store))

		super.init()

		configureStatusItem()
		configurePanel()
		observePanelLifecycle()
	}

	deinit {
		NotificationCenter.default.removeObserver(self)
	}

	@objc
	private func togglePanel() {
		if panel.isVisible {
			orderPanelOut()
		} else {
			showPanel()
		}
	}

	private func showPanel() {
		hostingView.layoutSubtreeIfNeeded()
		let fittingSize = hostingView.fittingSize
		if fittingSize.width > 0, fittingSize.height > 0 {
			panel.setContentSize(fittingSize)
		}
		positionPanel()
		if NSApp.isActive == false {
			// This is an explicit status-item click. Accessory apps do not
			// reliably become active from cooperative activation before the panel
			// is ordered front, so force activation for this explicit user-initiated
			// presentation only. The close path never activates the app.
			NSApp.activate(ignoringOtherApps: true)
		}
		panel.makeKeyAndOrderFront(nil)
		store.requestRefresh()
	}

	private func orderPanelOut() {
		panel.orderOut(nil)
	}

	private func configureStatusItem() {
		guard let button = statusItem.button else {
			return
		}

		button.image = AppAssets.statusBarIcon
		button.imagePosition = .imageOnly
		button.target = self
		button.action = #selector(togglePanel)
		button.sendAction(on: [.leftMouseUp])
		button.toolTip = "Decodex"
		button.setAccessibilityLabel("Decodex")
	}

	private func configurePanel() {
		hostingView.wantsLayer = true
		hostingView.layer?.backgroundColor = NSColor.clear.cgColor
		panel.isReleasedWhenClosed = false
		panel.isOpaque = false
		panel.backgroundColor = .clear
		panel.hasShadow = false
		// Let NSPanel own the transient presentation lifecycle. Keeping this true
		// prevents a panel shown by the status item from surviving app focus
		// changes and competing with the next status-item click.
		panel.hidesOnDeactivate = true
		panel.isMovable = false
		panel.level = .popUpMenu
		panel.collectionBehavior = [
			.transient,
			.moveToActiveSpace,
			.fullScreenAuxiliary,
		]
		panel.contentView = hostingView
		PanelWindowAppearance.apply(to: panel)
	}

	private func observePanelLifecycle() {
		NotificationCenter.default.addObserver(
			self,
			selector: #selector(panelDidResize(_:)),
			name: NSWindow.didResizeNotification,
			object: panel
		)
	}

	@objc
	private func panelDidResize(_: Notification) {
		guard panel.isVisible else {
			return
		}
		positionPanel()
	}

	private func positionPanel() {
		guard let anchorRect = statusItemScreenRect(),
			let statusWindow = statusItem.button?.window
		else {
			return
		}

		let screens = NSScreen.screens
		let fallbackScreen = statusWindow.screen ?? NSScreen.main
		let fallbackIndex = fallbackScreen.flatMap { fallbackScreen in
			screens.firstIndex(where: { $0 === fallbackScreen })
		}
		guard let screenIndex = StatusPanelLayout.screenIndex(
			containing: anchorRect,
			screenFrames: screens.map(\.frame),
			fallbackIndex: fallbackIndex
		) else {
			return
		}
		let screen = screens[screenIndex]

		panel.setFrameOrigin(
			StatusPanelLayout.origin(
				anchorRect: anchorRect,
				panelSize: panel.frame.size,
				visibleFrame: screen.visibleFrame,
				screenMargin: Self.screenMargin,
				menuBarGap: Self.menuBarGap
			)
		)
	}

	private func statusItemScreenRect() -> NSRect? {
		guard let button = statusItem.button,
			let statusWindow = button.window
		else {
			return nil
		}
		return statusWindow.convertToScreen(
			button.convert(button.bounds, to: nil)
		)
	}
}

@MainActor
final class TransparentHostingView<Content: View>: NSHostingView<Content> {
	override var isOpaque: Bool {
		false
	}
}

enum StatusPanelLayout {
	static func screenIndex(
		containing anchorRect: NSRect,
		screenFrames: [NSRect],
		fallbackIndex: Int?
	) -> Int? {
		let anchorCenter = NSPoint(x: anchorRect.midX, y: anchorRect.midY)
		if let containingIndex = screenFrames.firstIndex(where: {
			$0.contains(anchorCenter)
		}) {
			return containingIndex
		}

		if let intersectingIndex = screenFrames.firstIndex(where: {
			$0.intersects(anchorRect)
		}) {
			return intersectingIndex
		}

		if let fallbackIndex, screenFrames.indices.contains(fallbackIndex) {
			return fallbackIndex
		}

		return screenFrames.indices.first
	}

	static func origin(
		anchorRect: NSRect,
		panelSize: NSSize,
		visibleFrame: NSRect,
		screenMargin: CGFloat = 8,
		menuBarGap: CGFloat = 4
	) -> NSPoint {
		let minimumX = visibleFrame.minX + screenMargin
		let maximumX = visibleFrame.maxX - panelSize.width - screenMargin
		let proposedX = anchorRect.midX - panelSize.width / 2

		let minimumY = visibleFrame.minY + screenMargin
		let maximumY = visibleFrame.maxY - panelSize.height - screenMargin
		let proposedY = anchorRect.minY - panelSize.height - menuBarGap

		return NSPoint(
			x: clamped(proposedX, minimum: minimumX, maximum: maximumX),
			y: clamped(proposedY, minimum: minimumY, maximum: maximumY)
		)
	}

	private static func clamped(
		_ value: CGFloat,
		minimum: CGFloat,
		maximum: CGFloat
	) -> CGFloat {
		guard minimum <= maximum else {
			return minimum
		}
		return min(max(value, minimum), maximum)
	}
}

@MainActor
final class TransparentStatusPanel: NSPanel {
	override var canBecomeKey: Bool {
		true
	}

	override var canBecomeMain: Bool {
		false
	}
}

private struct StatusPanelRootView: View {
	let store: ResetCardStore

	var body: some View {
		AccountPanelView(store: store)
	}
}
