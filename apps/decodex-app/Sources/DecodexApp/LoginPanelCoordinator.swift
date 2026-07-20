import AppKit
import SwiftUI

struct LoginPanelPresenter: NSViewRepresentable {
	let store: AccountStore
	let state: LoginWindowState
	let isPresented: Bool
	let mode: AccountLoginSheetMode

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
		context.coordinator.update(
			store: store,
			state: state,
			isPresented: isPresented,
			mode: mode
		)
	}
}

final class LoginFloatingPanel: NSPanel {
	override var canBecomeKey: Bool {
		true
	}

	override var canBecomeMain: Bool {
		false
	}
}

final class LoginPanelPlacementState {
	weak var lastParent: NSWindow?
	var lastPanelSize: NSSize?
	var hasPlacedPanel = false

	func reset() {
		lastParent = nil
		lastPanelSize = nil
		hasPlacedPanel = false
	}

	func shouldPlace(
		parentWindow: NSWindow?,
		panelSize: NSSize,
		forcePlacement: Bool
	) -> Bool {
		let parentChanged = Self.windowsDiffer(lastParent, parentWindow)
		let sizeChanged = lastPanelSize.map {
			LoginPanelLayout.sizeDiffers($0, panelSize)
		} ?? true

		return forcePlacement || parentChanged || sizeChanged
	}

	func markPlaced(parentWindow: NSWindow?, panelSize: NSSize) {
		lastParent = parentWindow
		lastPanelSize = panelSize
		hasPlacedPanel = true
	}

	func parentDiffers(from window: NSWindow?) -> Bool {
		Self.windowsDiffer(lastParent, window)
	}

	private static func windowsDiffer(_ lhs: NSWindow?, _ rhs: NSWindow?) -> Bool {
		switch (lhs, rhs) {
		case (nil, nil):
			return false
		case (.some(let lhs), .some(let rhs)):
			return lhs !== rhs
		default:
			return true
		}
	}
}

extension LoginPanelPresenter {
	final class Coordinator: NSObject, NSWindowDelegate {
		weak var hostView: NSView?
		private weak var state: LoginWindowState?
		private var panel: LoginFloatingPanel?
		private var hostingView: NSHostingView<LoginSheetView>?
		private let placementState = LoginPanelPlacementState()
		private var isClosingProgrammatically = false

		@MainActor
		func update(
			store: AccountStore,
			state: LoginWindowState,
			isPresented: Bool,
			mode: AccountLoginSheetMode
		) {
			self.state = state

			guard isPresented else {
				closePanel()
				return
			}

			let rootView = makeRootView(store: store, state: state, mode: mode)
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
			configure(panel, hostingView: hostingView)

			self.panel = panel
			self.hostingView = hostingView
			resizeAndPlace(panel, hostingView: hostingView, forcePlacement: true)
			show(panel)
		}

		func windowWillClose(_ notification: Notification) {
			if let panel {
				panel.parent?.removeChildWindow(panel)
			}
			panel = nil
			hostingView = nil
			placementState.reset()
			guard isClosingProgrammatically == false else {
				return
			}

			Task { @MainActor [weak state] in
				state?.isPresented = false
			}
		}

		@MainActor
		private func makeRootView(
			store: AccountStore,
			state: LoginWindowState,
			mode: AccountLoginSheetMode
		) -> LoginSheetView {
			LoginSheetView(
				store: store,
				mode: mode,
				onCancel: { [weak state] in
					state?.isPresented = false
				},
				onComplete: { [weak state] in
					state?.isPresented = false
				}
			)
		}

		@MainActor
		private func configure(_ panel: LoginFloatingPanel, hostingView: NSHostingView<LoginSheetView>) {
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
			placementState.reset()
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
			guard placementState.shouldPlace(
				parentWindow: parentWindow,
				panelSize: panelSize,
				forcePlacement: forcePlacement
			) else {
				return
			}

			let currentFrame = placementState.hasPlacedPanel ? panel.frame : nil
			let frame = NSRect(
				origin: origin(for: panelSize, parentWindow: parentWindow, currentFrame: currentFrame),
				size: panelSize
			)
			panel.setFrame(frame, display: true)
			placementState.markPlaced(parentWindow: parentWindow, panelSize: panelSize)
		}

		@MainActor
		@discardableResult
		private func show(_ panel: NSPanel) -> Bool {
			guard let parentWindow = hostView?.window else {
				let hadParent = panel.parent != nil
				panel.parent?.removeChildWindow(panel)
				panel.orderFrontRegardless()
				return hadParent || placementState.parentDiffers(from: nil)
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
	}
}
