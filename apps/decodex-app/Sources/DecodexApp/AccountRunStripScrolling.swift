import AppKit
import Foundation
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

private final class AccountRunStripNSScrollView: NSScrollView {
	var onScrollWheelEvent: ((NSEvent) -> Bool)?

	override func scrollWheel(with event: NSEvent) {
		if onScrollWheelEvent?(event) == true {
			return
		}

		super.scrollWheel(with: event)
	}
}

private final class AccountRunStripClipView: NSClipView {
	var onBoundsChange: (() -> Void)?

	override func constrainBoundsRect(_ proposedBounds: NSRect) -> NSRect {
		var constrainedBounds = super.constrainBoundsRect(proposedBounds)
		constrainedBounds.origin.x = max(0, constrainedBounds.origin.x)
		constrainedBounds.origin.y = max(0, constrainedBounds.origin.y)

		return constrainedBounds
	}

	override func scroll(to newOrigin: NSPoint) {
		let oldOrigin = bounds.origin

		super.scroll(to: newOrigin)
		publishIfNeeded(from: oldOrigin)
	}

	override func setBoundsOrigin(_ newOrigin: NSPoint) {
		let oldOrigin = bounds.origin

		super.setBoundsOrigin(newOrigin)
		publishIfNeeded(from: oldOrigin)
	}

	private func publishIfNeeded(from oldOrigin: NSPoint) {
		guard oldOrigin != bounds.origin else {
			return
		}

		onBoundsChange?()
	}
}

private final class AccountRunDragHostingView<Content: View>: NSHostingView<Content> {
	weak var dragScrollView: NSScrollView?
	var onDragScroll: (() -> Void)?
	var onClick: ((NSPoint) -> Void)?
	private var dragStartPoint: NSPoint?
	private var dragStartOffset: CGFloat = 0
	private var isDraggingContent = false

	override func resetCursorRects() {
		super.resetCursorRects()

		addCursorRect(bounds, cursor: canDrag ? .openHand : .arrow)
	}

	override func mouseDown(with event: NSEvent) {
		guard canDrag, let dragScrollView else {
			super.mouseDown(with: event)
			return
		}

		dragStartPoint = convert(event.locationInWindow, from: nil)
		dragStartOffset = dragScrollView.contentView.bounds.origin.x
		isDraggingContent = false
		NSCursor.openHand.set()
	}

	override func mouseDragged(with event: NSEvent) {
		guard
			let dragStartPoint,
			let dragScrollView,
			canDrag
		else {
			super.mouseDragged(with: event)
			return
		}

		let currentPoint = convert(event.locationInWindow, from: nil)
		let deltaX = currentPoint.x - dragStartPoint.x
		if abs(deltaX) > AccountRunStripLayout.dragActivationDistance {
			isDraggingContent = true
		}
		guard isDraggingContent else {
			return
		}

		NSCursor.closedHand.set()
		scroll(dragScrollView, to: dragStartOffset - deltaX)
	}

	override func mouseUp(with event: NSEvent) {
		guard canDrag else {
			super.mouseUp(with: event)
			return
		}

		if isDraggingContent == false {
			onClick?(convert(event.locationInWindow, from: nil))
		}

		dragStartPoint = nil
		isDraggingContent = false
		NSCursor.openHand.set()
	}

	private var canDrag: Bool {
		guard let dragScrollView else {
			return false
		}

		let contentWidth = dragScrollView.documentView?.frame.width ?? 0
		return contentWidth > dragScrollView.contentView.bounds.width + 1
	}

	private func scroll(_ scrollView: NSScrollView, to offset: CGFloat) {
		let clipView = scrollView.contentView
		let contentWidth = scrollView.documentView?.frame.width ?? 0
		let maxOffset = max(0, contentWidth - clipView.bounds.width)
		let clampedOffset = min(max(0, offset), maxOffset)

		clipView.scroll(to: NSPoint(x: clampedOffset, y: clipView.bounds.origin.y))
		scrollView.reflectScrolledClipView(clipView)
		onDragScroll?()
	}
}

private final class AccountRunContinuousScroller {
	private var frameAction: ((TimeInterval) -> Bool)?
	private var lastTickTime: TimeInterval?
	private var timer: Timer?
	private var timerTarget: AccountRunContinuousTimerTarget?

	deinit {
		stop()
	}

	func start(_ frameAction: @escaping (TimeInterval) -> Bool) {
		stop()
		self.frameAction = frameAction
		lastTickTime = ProcessInfo.processInfo.systemUptime

		let timerTarget = AccountRunContinuousTimerTarget(scroller: self)
		let timer = Timer(
			timeInterval: AccountRunStripLayout.continuousScrollTickInterval,
			target: timerTarget,
			selector: #selector(AccountRunContinuousTimerTarget.timerDidFire(_:)),
			userInfo: nil,
			repeats: true
		)
		self.timerTarget = timerTarget
		self.timer = timer
		RunLoop.main.add(timer, forMode: .common)
	}

	func stop() {
		timer?.invalidate()
		timer = nil
		timerTarget = nil
		frameAction = nil
		lastTickTime = nil
	}

	fileprivate func performFrame() {
		guard let frameAction else {
			return
		}

		let tickTime = ProcessInfo.processInfo.systemUptime
		let elapsedTime = lastTickTime.map { tickTime - $0 }
			?? AccountRunStripLayout.continuousScrollTickInterval
		lastTickTime = tickTime

		let boundedElapsedTime = min(
			max(elapsedTime, 0),
			AccountRunStripLayout.continuousScrollMaximumFrameInterval
		)
		if frameAction(boundedElapsedTime) == false {
			stop()
		}
	}
}

private final class AccountRunContinuousTimerTarget: NSObject {
	weak var scroller: AccountRunContinuousScroller?

	init(scroller: AccountRunContinuousScroller) {
		self.scroller = scroller
	}

	@objc func timerDidFire(_ timer: Timer) {
		scroller?.performFrame()
	}
}

final class AccountRunStripContainerView<Content: View>: NSView, AccountRunStripScrollable {
	fileprivate let scrollView = AccountRunStripNSScrollView()
	fileprivate let notifyingClipView = AccountRunStripClipView()
	fileprivate let continuousScroller = AccountRunContinuousScroller()
	fileprivate let hostingView: AccountRunDragHostingView<Content>
	let placementStore: AccountRunStripPlacementStore
	var measuredContentWidth: CGFloat = 0
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

extension AccountRunStripContainerView {
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

	func handleScrollWheel(_ event: NSEvent) -> Bool {
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

	func scrollClickedRunToLeadingEdge(at point: NSPoint) {
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

	func scroll(to offset: CGFloat, animated: Bool = false) {
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
}
