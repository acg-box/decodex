import AppKit
import Foundation
import SwiftUI

struct AccountRunStripScrollView<Content: View>: NSViewRepresentable {
	let placementStore: AccountRunStripPlacementStore
	let scrollProxy: AccountRunStripScrollProxy
	let allowsPointerPanning: Bool
	let onMetricsChange: (AccountRunStripMetrics) -> Void
	@ViewBuilder let content: () -> Content

	func makeCoordinator() -> Coordinator {
		Coordinator(onMetricsChange: onMetricsChange)
	}

	func makeNSView(context: Context) -> AccountRunStripContainerView<Content> {
		let view = AccountRunStripContainerView(
			rootView: content(),
			placementStore: placementStore,
			allowsPointerPanning: allowsPointerPanning
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
		nsView.allowsPointerPanning = allowsPointerPanning
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

final class AccountRunStripContainerView<Content: View>: NSView, AccountRunStripScrollable {
	let scrollView = AccountRunStripNSScrollView()
	let notifyingClipView = AccountRunStripClipView()
	let continuousScroller = AccountRunContinuousScroller()
	let hostingView: AccountRunDragHostingView<Content>
	let placementStore: AccountRunStripPlacementStore
	var measuredContentWidth: CGFloat = 0
	var onMetricsChange: ((AccountRunStripMetrics) -> Void)?
	var allowsPointerPanning: Bool {
		get {
			hostingView.allowsPointerPanning
		}
		set {
			hostingView.allowsPointerPanning = newValue
		}
	}

	init(
		rootView: Content,
		placementStore: AccountRunStripPlacementStore,
		allowsPointerPanning: Bool
	) {
		self.placementStore = placementStore
		hostingView = AccountRunDragHostingView(rootView: rootView)
		hostingView.allowsPointerPanning = allowsPointerPanning

		super.init(frame: .zero)

		scrollView.contentView = notifyingClipView
		scrollView.drawsBackground = false
		scrollView.borderType = .noBorder
		scrollView.hasHorizontalScroller = false
		scrollView.hasVerticalScroller = false
		scrollView.autohidesScrollers = true
		scrollView.scrollerStyle = .overlay
		scrollView.horizontalScrollElasticity = .none
		scrollView.verticalScrollElasticity = .none
		scrollView.onScrollWheelEvent = { [weak self] event in
			self?.handleScrollWheel(event) ?? false
		}
		notifyingClipView.onBoundsChange = { [weak self] in
			self?.publishMetrics()
		}

		hostingView.dragScrollView = scrollView
		hostingView.onDragScroll = { [weak self] in
			self?.publishMetrics()
		}
		hostingView.onClick = { [weak self] point in
			self?.scrollClickedRunToLeadingEdge(at: point)
		}

		scrollView.documentView = hostingView
		addSubview(scrollView)
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("init(coder:) has not been implemented")
	}

	override var isFlipped: Bool {
		true
	}

	var clipView: NSClipView {
		scrollView.contentView
	}

	var currentMetrics: AccountRunStripMetrics {
		AccountRunStripMetrics(
			contentWidth: measuredContentWidth,
			viewportWidth: clipView.bounds.width,
			offsetX: clipView.bounds.origin.x
		)
	}

	override func layout() {
		super.layout()

		scrollView.frame = bounds
		updateDocumentFrame()
		clampScrollOffset()
		publishMetrics()
		hostingView.window?.invalidateCursorRects(for: hostingView)
	}

	func update(rootView: Content) {
		hostingView.rootView = rootView
		hostingView.invalidateIntrinsicContentSize()
		needsLayout = true
		layoutSubtreeIfNeeded()
		publishMetrics()
	}

	func updateDocumentFrame() {
		let contentSize = hostingView.fittingSize
		let height = max(bounds.height, AccountRunChipLayout.height)
		measuredContentWidth = max(0, ceil(contentSize.width))
		let documentWidth = max(measuredContentWidth, 1)

		hostingView.frame = NSRect(
			x: 0,
			y: 0,
			width: documentWidth,
			height: height
		)
	}

	func clampScrollOffset() {
		let maxOffset = max(0, measuredContentWidth - clipView.bounds.width)
		let currentOffset = clipView.bounds.origin.x
		let clampedOffset = min(max(0, currentOffset), maxOffset)

		guard clampedOffset != currentOffset else {
			return
		}

		scroll(to: clampedOffset)
	}

	func publishMetrics() {
		onMetricsChange?(currentMetrics)
	}
}
