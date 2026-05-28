import AppKit
import SwiftUI

struct LoginPanelPresenter: NSViewRepresentable {
	@ObservedObject var store: AccountStore
	@ObservedObject var state: LoginWindowState

	func makeCoordinator() -> Coordinator {
		Coordinator()
	}

	func makeNSView(context: Context) -> NSView {
		let view = NSView(frame: .zero)
		context.coordinator.hostView = view

		return view
	}

	func updateNSView(_ nsView: NSView, context: Context) {
		context.coordinator.hostView = nsView
		context.coordinator.update(store: store, state: state)
	}

	final class Coordinator: NSObject, NSWindowDelegate {
		weak var hostView: NSView?
		private weak var state: LoginWindowState?
		private var panel: LoginFloatingPanel?
		private var hostingView: NSHostingView<LoginSheetView>?
		private weak var lastPlacementParent: NSWindow?
		private var lastPanelSize: NSSize?
		private var hasPlacedPanel = false
		private var isClosingProgrammatically = false

		@MainActor
		func update(store: AccountStore, state: LoginWindowState) {
			self.state = state

			guard state.isPresented else {
				closePanel()
				return
			}

			let rootView = makeRootView(store: store, state: state)
			if let panel, let hostingView {
				hostingView.rootView = rootView
				let parentChanged = show(panel)
				resizeAndPlace(panel, hostingView: hostingView, forcePlacement: parentChanged)
				return
			}

			let hostingView = NSHostingView(rootView: rootView)
			hostingView.frame = NSRect(origin: .zero, size: NSSize(width: 328, height: 220))

			let panel = LoginFloatingPanel(
				contentRect: hostingView.frame,
				styleMask: [.borderless, .nonactivatingPanel],
				backing: .buffered,
				defer: false
			)
			panel.delegate = self
			panel.contentView = hostingView
			panel.backgroundColor = .clear
			panel.isOpaque = false
			panel.hasShadow = true
			panel.isFloatingPanel = true
			panel.hidesOnDeactivate = false
			panel.becomesKeyOnlyIfNeeded = true
			panel.isReleasedWhenClosed = false
			panel.level = .floating
			panel.animationBehavior = .utilityWindow
			panel.collectionBehavior = [.transient, .fullScreenAuxiliary, .canJoinAllSpaces]

			self.panel = panel
			self.hostingView = hostingView
			resizeAndPlace(panel, hostingView: hostingView, forcePlacement: true)
			show(panel)
		}

		@MainActor
		private func makeRootView(store: AccountStore, state: LoginWindowState) -> LoginSheetView {
			LoginSheetView(
				store: store,
				mode: state.mode,
				onCancel: { [weak state] in
					state?.isPresented = false
				},
				onComplete: { [weak state] in
					state?.isPresented = false
				}
			)
		}

		@MainActor
		private func closePanel() {
			guard let panel else {
				return
			}

			isClosingProgrammatically = true
			panel.parent?.removeChildWindow(panel)
			panel.close()
			isClosingProgrammatically = false
			self.panel = nil
			hostingView = nil
			lastPlacementParent = nil
			lastPanelSize = nil
			hasPlacedPanel = false
		}

		@MainActor
		private func resizeAndPlace(
			_ panel: NSPanel,
			hostingView: NSHostingView<LoginSheetView>,
			forcePlacement: Bool = false
		) {
			hostingView.layoutSubtreeIfNeeded()
			let fittingSize = hostingView.fittingSize
			let panelSize = LoginPanelLayout.panelSize(for: fittingSize)
			let parentWindow = hostView?.window
			let parentChanged = windowsDiffer(lastPlacementParent, parentWindow)
			let sizeChanged = lastPanelSize.map { LoginPanelLayout.sizeDiffers($0, panelSize) } ?? true
			guard forcePlacement || parentChanged || sizeChanged else {
				return
			}

			let currentFrame = hasPlacedPanel ? panel.frame : nil
			let frame = NSRect(
				origin: origin(for: panelSize, parentWindow: parentWindow, currentFrame: currentFrame),
				size: panelSize
			)
			panel.setFrame(frame, display: true)
			lastPlacementParent = parentWindow
			lastPanelSize = panelSize
			hasPlacedPanel = true
		}

		@MainActor
		@discardableResult
		private func show(_ panel: NSPanel) -> Bool {
			guard let parentWindow = hostView?.window else {
				let hadParent = panel.parent != nil
				panel.parent?.removeChildWindow(panel)
				panel.orderFrontRegardless()
				return hadParent || windowsDiffer(lastPlacementParent, nil)
			}

			let parentChanged = panel.parent !== parentWindow
			if panel.parent !== parentWindow {
				panel.parent?.removeChildWindow(panel)
				parentWindow.addChildWindow(panel, ordered: .above)
			}
			panel.level = parentWindow.level
			panel.orderFrontRegardless()
			return parentChanged
		}

		@MainActor
		private func origin(
			for panelSize: NSSize,
			parentWindow: NSWindow?,
			currentFrame: NSRect?
		) -> NSPoint {
			let screen = parentWindow?.screen ?? NSScreen.main
			return LoginPanelLayout.origin(
				for: panelSize,
				parentFrame: parentWindow?.frame,
				currentFrame: currentFrame,
				mouseLocation: NSEvent.mouseLocation,
				visibleFrame: screen?.visibleFrame ?? LoginPanelLayout.fallbackVisibleFrame
			)
		}

		private func windowsDiffer(_ lhs: NSWindow?, _ rhs: NSWindow?) -> Bool {
			switch (lhs, rhs) {
			case (nil, nil):
				return false
			case (.some(let lhs), .some(let rhs)):
				return lhs !== rhs
			default:
				return true
			}
		}

		func windowWillClose(_ notification: Notification) {
			if let panel {
				panel.parent?.removeChildWindow(panel)
			}
			panel = nil
			hostingView = nil
			lastPlacementParent = nil
			lastPanelSize = nil
			hasPlacedPanel = false
			guard isClosingProgrammatically == false else {
				return
			}

			Task { @MainActor [weak state] in
				state?.isPresented = false
			}
		}
	}
}

enum LoginPanelLayout {
	static let minimumSize = NSSize(width: 328, height: 190)
	static let fallbackVisibleFrame = NSRect(x: 0, y: 0, width: 1_280, height: 800)
	private static let parentTopOffset: CGFloat = 68
	private static let mouseTopOffset: CGFloat = 18
	private static let placementMargin: CGFloat = 8
	private static let sizeTolerance: CGFloat = 0.5

	static func panelSize(for fittingSize: NSSize) -> NSSize {
		NSSize(
			width: ceil(max(minimumSize.width, fittingSize.width)),
			height: ceil(max(minimumSize.height, fittingSize.height))
		)
	}

	static func origin(
		for panelSize: NSSize,
		parentFrame: NSRect?,
		currentFrame: NSRect?,
		mouseLocation: NSPoint,
		visibleFrame: NSRect
	) -> NSPoint {
		let proposedOrigin: NSPoint
		if let parentFrame {
			proposedOrigin = NSPoint(
				x: parentFrame.midX - panelSize.width / 2,
				y: parentFrame.maxY - panelSize.height - parentTopOffset
			)
		} else if let currentFrame {
			proposedOrigin = NSPoint(
				x: currentFrame.midX - panelSize.width / 2,
				y: currentFrame.maxY - panelSize.height
			)
		} else {
			proposedOrigin = NSPoint(
				x: mouseLocation.x - panelSize.width / 2,
				y: mouseLocation.y - panelSize.height - mouseTopOffset
			)
		}

		return NSPoint(
			x: clamped(
				proposedOrigin.x,
				min: visibleFrame.minX + placementMargin,
				max: visibleFrame.maxX - panelSize.width - placementMargin
			),
			y: clamped(
				proposedOrigin.y,
				min: visibleFrame.minY + placementMargin,
				max: visibleFrame.maxY - panelSize.height - placementMargin
			)
		)
	}

	static func sizeDiffers(_ lhs: NSSize, _ rhs: NSSize) -> Bool {
		abs(lhs.width - rhs.width) > sizeTolerance || abs(lhs.height - rhs.height) > sizeTolerance
	}

	private static func clamped(_ value: CGFloat, min: CGFloat, max: CGFloat) -> CGFloat {
		Swift.min(Swift.max(value, min), max)
	}
}

private final class LoginFloatingPanel: NSPanel {
	override var canBecomeKey: Bool {
		true
	}

	override var canBecomeMain: Bool {
		false
	}
}
