import AppKit
import SwiftUI

private final class AccountRunStripNSScrollView: NSScrollView {
	var onScrollWheelEvent: ((NSEvent) -> Bool)?

	override func scrollWheel(with event: NSEvent) {
		if onScrollWheelEvent?(event) == true {
			return
		}

		super.scrollWheel(with: event)
	}
}

final class AccountRunStripContainerView<Content: View>: NSView, AccountRunStripScrollable {
	private let scrollView = AccountRunStripNSScrollView()
	private let notifyingClipView = AccountRunStripClipView()
	private let continuousScroller = AccountRunContinuousScroller()
	private let hostingView: AccountRunDragHostingView<Content>
	private let placementStore: AccountRunStripPlacementStore
	private var measuredContentWidth: CGFloat = 0
	var onMetricsChange: ((AccountRunStripMetrics) -> Void)?

	init(rootView: Content, placementStore: AccountRunStripPlacementStore) {
		self.placementStore = placementStore
		hostingView = AccountRunDragHostingView(rootView: rootView)

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

	func scroll(by distance: CGFloat) {
		scroll(to: clipView.bounds.origin.x + distance)
	}

	func scrollToAdjacentRun(_ direction: AccountRunStripScrollDirection) {
		layoutSubtreeIfNeeded()

		guard let offset = adjacentRunOffset(for: direction) else {
			return
		}

		scroll(to: offset, animated: true)
	}

	func startContinuousScroll(_ direction: AccountRunStripScrollDirection) {
		layoutSubtreeIfNeeded()

		guard measuredContentWidth > clipView.bounds.width + 1 else {
			return
		}

		continuousScroller.start { [weak self] elapsedTime in
			guard let self else {
				return false
			}

			let previousOffset = self.clipView.bounds.origin.x
			let distance = direction.scrollMultiplier
				* AccountRunStripLayout.continuousScrollVelocity
				* CGFloat(elapsedTime)
			self.scroll(by: distance)

			return previousOffset != self.clipView.bounds.origin.x
		}
	}

	func stopContinuousScroll() {
		continuousScroller.stop()
	}

	private func handleScrollWheel(_ event: NSEvent) -> Bool {
		layoutSubtreeIfNeeded()

		guard measuredContentWidth > clipView.bounds.width + 1 else {
			return false
		}

		let distance = wheelScrollDistance(from: event)
		guard abs(distance) > AccountRunStripLayout.wheelMinimumDelta else {
			return false
		}

		let previousOffset = clipView.bounds.origin.x
		scroll(by: distance)

		return previousOffset != clipView.bounds.origin.x
	}

	private func scrollClickedRunToLeadingEdge(at point: NSPoint) {
		layoutSubtreeIfNeeded()

		guard
			measuredContentWidth > clipView.bounds.width + 1,
			let runID = placementStore.runID(containing: point),
			let frame = placementStore.frame(for: runID)
		else {
			return
		}

		scroll(to: frame.minX, animated: true)
	}

	private func adjacentRunOffset(for direction: AccountRunStripScrollDirection) -> CGFloat? {
		let maxOffset = max(0, measuredContentWidth - clipView.bounds.width)
		guard maxOffset > 0 else {
			return nil
		}

		let currentOffset = clipView.bounds.origin.x
		let orderedOffsets = placementStore.orderedFrames().map(\.minX)
		let targetOffset: CGFloat?
		switch direction {
		case .backward:
			targetOffset = orderedOffsets.last { offset in
				offset < currentOffset - 1
			} ?? (currentOffset > 0 ? 0 : nil)
		case .forward:
			targetOffset = orderedOffsets.first { offset in
				offset > currentOffset + 1
			} ?? (currentOffset < maxOffset ? maxOffset : nil)
		}

		return targetOffset.map { min(max(0, $0), maxOffset) }
	}

	private func wheelScrollDistance(from event: NSEvent) -> CGFloat {
		let rawDeltaX = event.scrollingDeltaX == 0 ? event.deltaX : event.scrollingDeltaX
		let rawDeltaY = event.scrollingDeltaY == 0 ? event.deltaY : event.scrollingDeltaY
		let deltaX = scaledWheelDelta(rawDeltaX, isPrecise: event.hasPreciseScrollingDeltas)
		let deltaY = scaledWheelDelta(rawDeltaY, isPrecise: event.hasPreciseScrollingDeltas)
		let dominantDelta = abs(deltaX) >= abs(deltaY) ? deltaX : deltaY

		return -dominantDelta
	}

	private func scaledWheelDelta(_ delta: CGFloat, isPrecise: Bool) -> CGFloat {
		isPrecise ? delta : delta * AccountRunStripLayout.wheelLineDeltaScale
	}

	private func updateDocumentFrame() {
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

	private func clampScrollOffset() {
		let maxOffset = max(0, measuredContentWidth - clipView.bounds.width)
		let currentOffset = clipView.bounds.origin.x
		let clampedOffset = min(max(0, currentOffset), maxOffset)

		guard clampedOffset != currentOffset else {
			return
		}

		scroll(to: clampedOffset)
	}

	private func scroll(to offset: CGFloat, animated: Bool = false) {
		layoutSubtreeIfNeeded()

		let maxOffset = max(0, measuredContentWidth - clipView.bounds.width)
		let clampedOffset = min(max(0, offset), maxOffset)
		guard clampedOffset != clipView.bounds.origin.x else {
			return
		}

		if animated {
			animateScroll(to: clampedOffset)
			return
		}

		clipView.scroll(to: NSPoint(x: clampedOffset, y: clipView.bounds.origin.y))
		scrollView.reflectScrolledClipView(clipView)
		publishMetrics()
	}

	private func animateScroll(to offset: CGFloat) {
		NSAnimationContext.runAnimationGroup { context in
			context.duration = AccountRunStripLayout.clickScrollDuration
			context.allowsImplicitAnimation = true
			clipView.animator().setBoundsOrigin(NSPoint(x: offset, y: clipView.bounds.origin.y))
		}
		scrollView.reflectScrolledClipView(clipView)
		publishMetrics()
	}

	private func publishMetrics() {
		onMetricsChange?(currentMetrics)
	}
}
