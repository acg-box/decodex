import AppKit
import Foundation
import SwiftUI

enum AccountRunChipLayout {
	static let height: CGFloat = 18.5
	static let cornerRadius: CGFloat = 9.25
	static let horizontalPadding: CGFloat = 6.5
	static let iconWidth: CGFloat = 9.5
	static let spacing: CGFloat = 4
	static let popoverHoverDelayNanoseconds: UInt64 = 320_000_000
}

enum AccountRunStripLayout {
	static let contentCoordinateSpace = "account-run-strip-content"
	static let dragActivationDistance: CGFloat = 1
	static let edgeControlSpacing: CGFloat = 4
	static let edgeControlWidth: CGFloat = 12
	static let edgeControlReservedWidth = edgeControlWidth * 2 + edgeControlSpacing * 2
	static let fadeWidth: CGFloat = 24
	static let overflowTolerance: CGFloat = 1
	static let wheelLineDeltaScale: CGFloat = 11
	static let wheelMinimumDelta: CGFloat = 0.1
	static let clickScrollDuration: TimeInterval = 0.14
	static let continuousScrollStartDelayNanoseconds: UInt64 = 200_000_000
	static let continuousScrollTickInterval: TimeInterval = 1.0 / 120.0
	static let continuousScrollMaximumFrameInterval: TimeInterval = 1.0 / 20.0
	static let continuousScrollVelocity: CGFloat = 285
}

enum AccountRunStripScrollDirection {
	case backward
	case forward

	var scrollMultiplier: CGFloat {
		switch self {
		case .backward:
			return -1
		case .forward:
			return 1
		}
	}

	var symbol: String {
		switch self {
		case .backward:
			return "chevron.left"
		case .forward:
			return "chevron.right"
		}
	}

	var accessibilityLabel: String {
		switch self {
		case .backward:
			return "Previous running lane"
		case .forward:
			return "Next running lane"
		}
	}

	var disabledHelp: String {
		switch self {
		case .backward:
			return "Already at the first running lane"
		case .forward:
			return "Already at the last running lane"
		}
	}
}

struct AccountRunStripMetrics: Equatable {
	var contentWidth: CGFloat = 0
	var viewportWidth: CGFloat = 0
	var isOverflowing = false
	var canScrollBackward = false
	var canScrollForward = false

	init() {}

	init(contentWidth: CGFloat, viewportWidth: CGFloat, offsetX: CGFloat) {
		self.contentWidth = contentWidth
		self.viewportWidth = viewportWidth
		let maxOffsetX = max(0, contentWidth - viewportWidth)
		isOverflowing = contentWidth > viewportWidth + AccountRunStripLayout.overflowTolerance
		canScrollBackward = isOverflowing && offsetX > 1
		canScrollForward = isOverflowing && offsetX < maxOffsetX - 1
	}
}

final class AccountRunStripPlacementStore {
	private var framesByRunID = [String: CGRect]()

	func update(runID: String, frame: CGRect) {
		framesByRunID[runID] = frame
	}

	func retainOnly(_ runIDs: Set<String>) {
		framesByRunID = framesByRunID.filter { runIDs.contains($0.key) }
	}

	func frame(for runID: String) -> CGRect? {
		framesByRunID[runID]
	}

	func orderedFrames() -> [CGRect] {
		framesByRunID.values.sorted { left, right in
			if left.minX == right.minX {
				return left.width < right.width
			}

			return left.minX < right.minX
		}
	}

	func runID(containing point: NSPoint) -> String? {
		framesByRunID.first { _, frame in
			frame.contains(point)
		}?.key
	}
}

struct AccountRunChipView: View {
	let card: OperatorCurrentLaneCard
	@Environment(\.colorScheme) private var colorScheme
	@State private var isHovered = false
	@State private var showsPopover = false
	@State private var hoverPopoverTask: Task<Void, Never>?

	var body: some View {
		HStack(spacing: AccountRunChipLayout.spacing) {
			Image(systemName: symbol)
				.font(PanelFont.runChipIcon)
				.foregroundStyle(tint.opacity(colorScheme == .dark ? 0.88 : 0.76))
				.frame(width: AccountRunChipLayout.iconWidth)

			Text(chipTitle)
				.font(PanelFont.runChipTitle)
				.foregroundStyle(PanelPalette.primaryText(colorScheme).opacity(0.92))
				.lineLimit(1)
				.truncationMode(.middle)
				.fixedSize(horizontal: true, vertical: false)
		}
		.frame(height: AccountRunChipLayout.height)
		.padding(.horizontal, AccountRunChipLayout.horizontalPadding)
		.background {
			RoundedRectangle(cornerRadius: AccountRunChipLayout.cornerRadius, style: .continuous)
				.fill(isHovered ? tint.opacity(colorScheme == .dark ? 0.09 : 0.07) : Color.clear)
		}
		.modernGlassSurface(cornerRadius: AccountRunChipLayout.cornerRadius, depth: .row)
		.contentShape(RoundedRectangle(cornerRadius: AccountRunChipLayout.cornerRadius, style: .continuous))
		.onHover { hovering in
			isHovered = hovering
			if hovering {
				schedulePopover()
			} else {
				cancelPopover()
			}
		}
		.onDisappear {
			cancelPopover()
		}
		.popover(isPresented: $showsPopover, arrowEdge: .trailing) {
			TimelineView(.periodic(from: Date(), by: 1)) { timeline in
				OperatorLanePopoverView(run: card.run, currentTime: timeline.date)
					.fixedSize(horizontal: true, vertical: false)
			}
		}
	}

	private func schedulePopover() {
		hoverPopoverTask?.cancel()
		hoverPopoverTask = Task {
			try? await Task.sleep(nanoseconds: AccountRunChipLayout.popoverHoverDelayNanoseconds)
			guard Task.isCancelled == false else {
				return
			}

			await MainActor.run {
				if isHovered {
					showsPopover = true
				}
			}
		}
	}

	private func cancelPopover() {
		hoverPopoverTask?.cancel()
		hoverPopoverTask = nil
		showsPopover = false
	}

	private var symbol: String {
		if card.needsAttention || card.tone == "attention" {
			return "exclamationmark.triangle.fill"
		}
		if card.isWaiting || card.tone == "waiting" {
			return "clock"
		}

		return "play.fill"
	}

	private var chipTitle: String {
		panelTrimmed(card.title) ?? panelTrimmed(card.issueIdentifier) ?? "Run"
	}

	private var tint: Color {
		if card.needsAttention || card.tone == "attention" {
			return PanelPalette.warning(colorScheme)
		}
		if card.isWaiting || card.tone == "waiting" {
			return PanelPalette.secondaryText(colorScheme)
		}

		return PanelPalette.routeAccent(colorScheme)
	}
}

struct AccountRunChipPlacementReporter: ViewModifier {
	let runID: String
	let placementStore: AccountRunStripPlacementStore

	func body(content: Content) -> some View {
		content.background {
			GeometryReader { proxy in
				Color.clear
					.onAppear {
						publish(proxy.frame(in: .named(AccountRunStripLayout.contentCoordinateSpace)))
					}
					.onChange(of: proxy.frame(in: .named(AccountRunStripLayout.contentCoordinateSpace))) { _, frame in
						publish(frame)
					}
			}
		}
	}

	private func publish(_ frame: CGRect) {
		DispatchQueue.main.async {
			placementStore.update(runID: runID, frame: frame)
		}
	}
}

struct AccountRunStripFadeMask: View {
	let metrics: AccountRunStripMetrics

	var body: some View {
		HStack(spacing: 0) {
			if metrics.canScrollBackward {
				LinearGradient(
					colors: [.clear, .black],
					startPoint: .leading,
					endPoint: .trailing
				)
				.frame(width: AccountRunStripLayout.fadeWidth)
			}

			Color.black

			if metrics.canScrollForward {
				LinearGradient(
					colors: [.black, .clear],
					startPoint: .leading,
					endPoint: .trailing
				)
				.frame(width: AccountRunStripLayout.fadeWidth)
			}
		}
	}
}

struct AccountRunStripEdgeButton: View {
	let direction: AccountRunStripScrollDirection
	let isEnabled: Bool
	let clickAction: () -> Void
	let startContinuousAction: () -> Void
	let stopContinuousAction: () -> Void
	@Environment(\.colorScheme) private var colorScheme
	@State private var isPressed = false
	@State private var pressTask: Task<Void, Never>?

	var body: some View {
		Image(systemName: direction.symbol)
			.font(.system(size: 10.5, weight: .semibold))
			.symbolRenderingMode(.monochrome)
			.foregroundStyle(tint)
			.scaleEffect(isEnabled && isPressed ? 0.92 : 1)
			.frame(
				width: AccountRunStripLayout.edgeControlWidth,
				height: AccountRunChipLayout.height
			)
			.contentShape(Rectangle())
			.allowsHitTesting(isEnabled)
			.highPriorityGesture(
				DragGesture(minimumDistance: 0)
					.onChanged { _ in
						startPress()
					}
					.onEnded { _ in
						endPress()
					}
			)
			.onDisappear {
				cancelPress()
			}
			.onChange(of: isEnabled) { _, isEnabled in
				if isEnabled == false {
					cancelPress()
				}
			}
			.help(isEnabled ? direction.accessibilityLabel : direction.disabledHelp)
			.accessibilityLabel(direction.accessibilityLabel)
			.accessibilityValue(isEnabled ? "Available" : "Unavailable")
	}

	private var tint: Color {
		let opacity: Double
		if isEnabled == false {
			opacity = colorScheme == .dark ? 0.28 : 0.22
		} else if isPressed {
			opacity = 0.92
		} else {
			opacity = colorScheme == .dark ? 0.62 : 0.5
		}

		return PanelPalette.primaryText(colorScheme).opacity(opacity)
	}

	private func startPress() {
		guard isEnabled, pressTask == nil else {
			return
		}

		isPressed = true
		clickAction()
		pressTask = Task {
			try? await Task.sleep(nanoseconds: AccountRunStripLayout.continuousScrollStartDelayNanoseconds)
			guard Task.isCancelled == false else {
				return
			}

			await MainActor.run {
				startContinuousAction()
			}
		}
	}

	private func endPress() {
		cancelPress()
	}

	private func cancelPress() {
		pressTask?.cancel()
		pressTask = nil
		stopContinuousAction()
		isPressed = false
	}
}

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

final class AccountRunStripNSScrollView: NSScrollView {
	var onScrollWheelEvent: ((NSEvent) -> Bool)?

	override func scrollWheel(with event: NSEvent) {
		if onScrollWheelEvent?(event) == true {
			return
		}

		super.scrollWheel(with: event)
	}
}

final class AccountRunStripClipView: NSClipView {
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

final class AccountRunDragHostingView<Content: View>: NSHostingView<Content> {
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

final class AccountRunContinuousScroller {
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
	let scrollView = AccountRunStripNSScrollView()
	let notifyingClipView = AccountRunStripClipView()
	let continuousScroller = AccountRunContinuousScroller()
	let hostingView: AccountRunDragHostingView<Content>
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

struct AccountRunSummaryView: View {
	let runs: [OperatorCurrentLaneCard]
	@State private var placementStore = AccountRunStripPlacementStore()
	@State private var scrollProxy = AccountRunStripScrollProxy()
	@State private var scrollMetrics = AccountRunStripMetrics()
	@State private var showsEdgeControls = false

	var body: some View {
		HStack(spacing: AccountRunStripLayout.edgeControlSpacing) {
			if showsEdgeControls {
				AccountRunStripEdgeButton(
					direction: .backward,
					isEnabled: scrollMetrics.canScrollBackward
				) {
					scrollProxy.scrollToAdjacentRun(.backward)
				} startContinuousAction: {
					scrollProxy.startContinuousScroll(.backward)
				} stopContinuousAction: {
					scrollProxy.stopContinuousScroll()
				}
				.transition(.panelInline)
			}

			AccountRunStripScrollView(
				placementStore: placementStore,
				scrollProxy: scrollProxy,
				onMetricsChange: { metrics in
					updateScrollMetrics(metrics)
				}
			) {
				HStack(spacing: 5) {
					ForEach(runs) { card in
						AccountRunChipView(card: card)
							.modifier(
								AccountRunChipPlacementReporter(
									runID: card.id,
									placementStore: placementStore
								)
							)
					}
				}
				.padding(.trailing, 1)
				.fixedSize(horizontal: true, vertical: false)
				.coordinateSpace(name: AccountRunStripLayout.contentCoordinateSpace)
			}
			.mask {
				AccountRunStripFadeMask(metrics: showsEdgeControls ? scrollMetrics : AccountRunStripMetrics())
			}
			.frame(maxWidth: .infinity, alignment: .leading)

			if showsEdgeControls {
				AccountRunStripEdgeButton(
					direction: .forward,
					isEnabled: scrollMetrics.canScrollForward
				) {
					scrollProxy.scrollToAdjacentRun(.forward)
				} startContinuousAction: {
					scrollProxy.startContinuousScroll(.forward)
				} stopContinuousAction: {
					scrollProxy.stopContinuousScroll()
				}
				.transition(.panelInline)
			}
		}
		.frame(height: AccountRunChipLayout.height)
		.frame(maxWidth: .infinity, alignment: .leading)
		.contentShape(Rectangle())
		.accessibilityLabel("\(runs.count) current lane\(runs.count == 1 ? "" : "s")")
		.onAppear {
			placementStore.retainOnly(Set(runs.map(\.id)))
		}
		.onChange(of: runs.map(\.id)) { _, runIDs in
			placementStore.retainOnly(Set(runIDs))
		}
		.animation(PanelMotion.inlineLayout, value: showsEdgeControls)
	}

	private func updateScrollMetrics(_ metrics: AccountRunStripMetrics) {
		let nextShowsEdgeControls = shouldShowEdgeControls(for: metrics)
		guard metrics != scrollMetrics || nextShowsEdgeControls != showsEdgeControls else {
			return
		}

		if showsEdgeControls && nextShowsEdgeControls == false {
			scrollProxy.stopContinuousScroll()
		}

		if metrics != scrollMetrics {
			var transaction = Transaction()
			transaction.disablesAnimations = true
			withTransaction(transaction) {
				scrollMetrics = metrics
			}
		}

		if nextShowsEdgeControls != showsEdgeControls {
			withAnimation(PanelMotion.inlineLayout) {
				showsEdgeControls = nextShowsEdgeControls
			}
		}
	}

	private func shouldShowEdgeControls(for metrics: AccountRunStripMetrics) -> Bool {
		let reservedWidth = showsEdgeControls ? AccountRunStripLayout.edgeControlReservedWidth : 0
		let viewportWidthWithoutEdgeControls = metrics.viewportWidth + reservedWidth

		return metrics.contentWidth > viewportWidthWithoutEdgeControls + AccountRunStripLayout.overflowTolerance
	}
}
