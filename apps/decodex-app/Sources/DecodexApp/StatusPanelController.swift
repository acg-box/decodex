import AppKit
import SwiftUI

@MainActor
final class StatusPanelController: NSObject {
	private static let screenMargin: CGFloat = 8
	private static let menuBarGap: CGFloat = 4

	private let statusItem: NSStatusItem
	private let panel: TransparentStatusPanel
	private let hostingView: NSHostingView<StatusPanelRootView>

	init(store: ResetCardStore) {
		statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
		panel = TransparentStatusPanel(
			contentRect: .zero,
			styleMask: [.borderless],
			backing: .buffered,
			defer: true
		)
		hostingView = NSHostingView(rootView: StatusPanelRootView(store: store))

		super.init()

		configureStatusItem()
		configurePanel()
	}

	@objc
	private func togglePanel() {
		if panel.isVisible {
			panel.orderOut(nil)
			return
		}

		hostingView.layoutSubtreeIfNeeded()
		let fittingSize = hostingView.fittingSize
		if fittingSize.width > 0, fittingSize.height > 0 {
			panel.setContentSize(fittingSize)
		}
		positionPanel()
		NSApp.activate(ignoringOtherApps: true)
		panel.makeKeyAndOrderFront(nil)
	}

	private func configureStatusItem() {
		guard let button = statusItem.button else {
			return
		}

		button.image = AppAssets.statusBarIcon
		button.imagePosition = .imageOnly
		button.target = self
		button.action = #selector(togglePanel)
		button.toolTip = "Decodex"
		button.setAccessibilityLabel("Decodex")
	}

	private func configurePanel() {
		panel.isReleasedWhenClosed = false
		panel.isOpaque = false
		panel.backgroundColor = .clear
		panel.hasShadow = false
		panel.hidesOnDeactivate = true
		panel.isMovable = false
		panel.level = .popUpMenu
		panel.collectionBehavior = [
			.transient,
			.moveToActiveSpace,
			.fullScreenAuxiliary,
		]
		panel.contentView = hostingView
	}

	private func positionPanel() {
		guard let button = statusItem.button,
			let statusWindow = button.window
		else {
			return
		}

		let anchorRect = statusWindow.convertToScreen(
			button.convert(button.bounds, to: nil)
		)
		guard let screen = statusWindow.screen
			?? NSScreen.screens.first(where: {
				$0.frame.intersects(anchorRect)
			})
			?? NSScreen.main
		else {
			return
		}

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
}

enum StatusPanelLayout {
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
