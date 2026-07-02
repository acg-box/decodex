import AppKit
import SwiftUI

struct AccountRunStripScrollView<Content: View>: NSViewRepresentable {
	let placementStore: AccountRunStripPlacementStore
	let scrollProxy: AccountRunStripScrollProxy
	let onMetricsChange: (AccountRunStripMetrics) -> Void
	@ViewBuilder let content: () -> Content

	func makeCoordinator() -> Coordinator {
		Coordinator(onMetricsChange: onMetricsChange)
	}

	func makeNSView(context: Context) -> AccountRunStripContainerView<Content> {
		let view = AccountRunStripContainerView(
			rootView: content(),
			placementStore: placementStore
		)
		scrollProxy.attach(view)
		view.onMetricsChange = { metrics in
			context.coordinator.publish(metrics)
		}

		return view
	}

	func updateNSView(_ nsView: AccountRunStripContainerView<Content>, context: Context) {
		context.coordinator.onMetricsChange = onMetricsChange
		scrollProxy.attach(nsView)
		nsView.onMetricsChange = { metrics in
			context.coordinator.publish(metrics)
		}
		nsView.update(rootView: content())
	}

	final class Coordinator {
		var onMetricsChange: (AccountRunStripMetrics) -> Void
		private var lastMetrics: AccountRunStripMetrics?

		init(onMetricsChange: @escaping (AccountRunStripMetrics) -> Void) {
			self.onMetricsChange = onMetricsChange
		}

		@MainActor
		func publish(_ metrics: AccountRunStripMetrics) {
			guard metrics != lastMetrics else {
				return
			}

			lastMetrics = metrics
			DispatchQueue.main.async { [onMetricsChange] in
				onMetricsChange(metrics)
			}
		}
	}
}

@MainActor
protocol AccountRunStripScrollable: AnyObject {
	func scrollToAdjacentRun(_ direction: AccountRunStripScrollDirection)
	func startContinuousScroll(_ direction: AccountRunStripScrollDirection)
	func stopContinuousScroll()
}

@MainActor
final class AccountRunStripScrollProxy {
	private weak var target: (any AccountRunStripScrollable)?

	func attach(_ target: any AccountRunStripScrollable) {
		self.target = target
	}

	func scrollToAdjacentRun(_ direction: AccountRunStripScrollDirection) {
		target?.scrollToAdjacentRun(direction)
	}

	func startContinuousScroll(_ direction: AccountRunStripScrollDirection) {
		target?.startContinuousScroll(direction)
	}

	func stopContinuousScroll() {
		target?.stopContinuousScroll()
	}
}
