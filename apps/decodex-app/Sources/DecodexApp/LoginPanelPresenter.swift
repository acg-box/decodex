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
				resizeAndPlace(panel, hostingView: hostingView)
				show(panel)
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
			resizeAndPlace(panel, hostingView: hostingView)
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
		}

		@MainActor
		private func resizeAndPlace(_ panel: NSPanel, hostingView: NSHostingView<LoginSheetView>) {
			hostingView.layoutSubtreeIfNeeded()
			let fittingSize = hostingView.fittingSize
			let panelSize = NSSize(
				width: max(328, fittingSize.width),
				height: max(190, fittingSize.height)
			)
			panel.setContentSize(panelSize)
			panel.setFrameOrigin(origin(for: panelSize))
		}

		@MainActor
		private func show(_ panel: NSPanel) {
			guard let parentWindow = hostView?.window else {
				panel.orderFrontRegardless()
				return
			}

			if panel.parent !== parentWindow {
				panel.parent?.removeChildWindow(panel)
				parentWindow.addChildWindow(panel, ordered: .above)
			}
			panel.level = parentWindow.level
			panel.orderFrontRegardless()
		}

		@MainActor
		private func origin(for panelSize: NSSize) -> NSPoint {
			let parentWindow = hostView?.window
			let screen = parentWindow?.screen ?? NSScreen.main
			let visibleFrame = screen?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1_280, height: 800)
			let margin: CGFloat = 8

			var x: CGFloat
			var y: CGFloat
			if let parentFrame = parentWindow?.frame {
				x = parentFrame.midX - panelSize.width / 2
				y = parentFrame.maxY - panelSize.height - 68
			} else {
				let mouse = NSEvent.mouseLocation
				x = mouse.x - panelSize.width / 2
				y = mouse.y - panelSize.height - 18
			}

			x = min(max(x, visibleFrame.minX + margin), visibleFrame.maxX - panelSize.width - margin)
			y = min(max(y, visibleFrame.minY + margin), visibleFrame.maxY - panelSize.height - margin)

			return NSPoint(x: x, y: y)
		}

		func windowWillClose(_ notification: Notification) {
			if let panel {
				panel.parent?.removeChildWindow(panel)
			}
			panel = nil
			hostingView = nil
			guard isClosingProgrammatically == false else {
				return
			}

			Task { @MainActor [weak state] in
				state?.isPresented = false
			}
		}
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
