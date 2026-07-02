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
}
