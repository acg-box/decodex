import AppKit
import SwiftUI
import QuartzCore

/// A compact strip that accepts both ordinary mouse wheels and trackpad swipes.
struct HorizontalCardScroller<Content: View>: NSViewRepresentable {
	@ViewBuilder let content: () -> Content

	func makeNSView(context: Context) -> CardScrollerView<Content> {
		CardScrollerView(content: content())
	}

	func updateNSView(_ view: CardScrollerView<Content>, context: Context) {
		view.host.rootView = content()
		view.needsLayout = true
	}
}

final class CardWheelScrollView: NSScrollView {
	static let animationKey = "decodex.card-scroll"
	var didScroll: (() -> Void)?
	private var animationGeneration: UInt64 = 0
	private var transition: CardScrollTransition?
	var reduceMotion: () -> Bool = { NSWorkspace.shared.accessibilityDisplayShouldReduceMotion }

	var isAnimating: Bool { contentView.layer?.animation(forKey: Self.animationKey) != nil }
	var visibleOffset: CGFloat {
		guard let animation = contentView.layer?.animation(forKey: Self.animationKey) as? CABasicAnimation else { return contentView.bounds.minX }
		return contentView.layer?.presentation()?.bounds.minX
			?? animation.fromValue as? CGFloat ?? contentView.bounds.minX
	}

	override func scrollWheel(with event: NSEvent) {
		guard (documentView?.frame.width ?? 0) > contentView.bounds.width else {
			super.scrollWheel(with: event)
			return
		}
		let delta = abs(event.scrollingDeltaX) > abs(event.scrollingDeltaY)
			? event.scrollingDeltaX : event.scrollingDeltaY
		move(by: -delta * (event.hasPreciseScrollingDeltas ? 1 : 12), animated: !event.hasPreciseScrollingDeltas)
	}

	func move(by delta: CGFloat, animated: Bool = false) {
		let maximum = max(0, (documentView?.frame.width ?? 0) - contentView.bounds.width)
		let start = visibleOffset
		let velocity = animated && isAnimating ? transition?.velocity(at: CACurrentMediaTime()) ?? 0 : 0
		let target = min(maximum, max(0, (animated ? contentView.bounds.minX : start) + delta))
		animationGeneration &+= 1
		let generation = animationGeneration
		transition = nil
		contentView.wantsLayer = true
		CATransaction.begin()
		CATransaction.setDisableActions(true)
		contentView.layer?.removeAnimation(forKey: Self.animationKey)
		contentView.scroll(to: NSPoint(x: target, y: 0))
		reflectScrolledClipView(contentView)
		if animated, !reduceMotion(), abs(target - start) > 0.5, window != nil {
			let animation = CABasicAnimation(keyPath: "bounds.origin.x")
			animation.fromValue = start
			animation.toValue = target
			animation.duration = CardScrollTransition.duration
			let tangent = min(1, max(0, Float(velocity * CardScrollTransition.duration / (3 * (target - start)))))
			animation.timingFunction = CAMediaTimingFunction(controlPoints: 1 / 3, tangent, 2 / 3, 1)
			transition = CardScrollTransition(start: start, target: target, began: CACurrentMediaTime(), initialVelocity: CGFloat(tangent) * 3 * (target - start) / CardScrollTransition.duration)
			if let screen = window?.screen {
				let rate = Float(screen.maximumFramesPerSecond)
				if rate > 0 { animation.preferredFrameRateRange = CAFrameRateRange(minimum: rate, maximum: rate, preferred: rate) }
			}
			animation.delegate = CardScrollCompletion(scroll: self, generation: generation)
			contentView.layer?.add(animation, forKey: Self.animationKey)
		}
		CATransaction.commit()
		didScroll?()
	}

	fileprivate func completed(_ generation: UInt64) {
		guard generation == animationGeneration else { return }
		transition = nil
		didScroll?()
	}

	override func mouseDown(with event: NSEvent) {
		if isAnimating {
			// The first click stops motion at the visible card, never at the target's hit area.
			move(by: 0)
			return
		}
		super.mouseDown(with: event)
	}

	override func viewDidMoveToWindow() {
		super.viewDidMoveToWindow()
		if window == nil { contentView.layer?.removeAnimation(forKey: Self.animationKey) }
	}
}

@MainActor
private final class CardScrollCompletion: NSObject, @preconcurrency CAAnimationDelegate {
	weak var scroll: CardWheelScrollView?
	let generation: UInt64

	init(scroll: CardWheelScrollView, generation: UInt64) {
		self.scroll = scroll
		self.generation = generation
	}

	func animationDidStop(_ animation: CAAnimation, finished: Bool) {
		if finished { scroll?.completed(generation) }
	}
}

final class CardContentHostingView<Content: View>: NSHostingView<Content> {
	weak var scroll: CardWheelScrollView?

	override func hitTest(_ point: NSPoint) -> NSView? {
		// Model geometry is at the destination while the presentation layer is moving.
		if let scroll, scroll.isAnimating { return scroll }
		return super.hitTest(point)
	}
}

final class CardScrollerView<Content: View>: NSView {
	let host: CardContentHostingView<Content>
	let scroll = CardWheelScrollView()
	let previous = CardArrowButton()
	let next = CardArrowButton()

	init(content: Content) {
		host = CardContentHostingView(rootView: content)
		super.init(frame: .zero)
		host.scroll = scroll
		scroll.drawsBackground = false
		scroll.borderType = .noBorder
		scroll.hasHorizontalScroller = false
		scroll.hasVerticalScroller = false
		scroll.horizontalScrollElasticity = .none
		scroll.verticalScrollElasticity = .none
		scroll.documentView = host
		addSubview(scroll)
		configure(previous, symbol: "chevron.left", label: "Previous Reset Cards", action: #selector(back))
		configure(next, symbol: "chevron.right", label: "Next Reset Cards", action: #selector(forward))
		scroll.didScroll = { [weak self] in self?.updateButtons() }
	}

	required init?(coder: NSCoder) { nil }

	private func configure(_ button: NSButton, symbol: String, label: String, action: Selector) {
		button.image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)
		button.imagePosition = .imageOnly
		button.isBordered = false
		button.controlSize = .mini
		button.setAccessibilityLabel(label)
		button.toolTip = label
		button.target = self
		button.action = action
		addSubview(button)
	}

	override func layout() {
		super.layout()
		let width = host.fittingSize.width
		let overflow = width > bounds.width
		let arrow: CGFloat = overflow ? 16 : 0
		previous.isHidden = !overflow
		next.isHidden = !overflow
		previous.frame = NSRect(x: 0, y: 0, width: arrow, height: bounds.height)
		next.frame = NSRect(x: bounds.width - arrow, y: 0, width: arrow, height: bounds.height)
		scroll.frame = NSRect(x: arrow, y: 0, width: max(0, bounds.width - arrow * 2), height: bounds.height)
		host.frame = NSRect(x: 0, y: 0, width: width, height: bounds.height)
		if scroll.contentView.bounds.minX > max(0, host.frame.width - scroll.contentView.bounds.width) { scroll.move(by: 0) }
		updateButtons()
	}

	private func updateButtons() {
		let canGoBack = scroll.contentView.bounds.minX > 0.5
		let canGoForward = scroll.contentView.bounds.maxX < host.frame.width - 0.5
		if previous.isEnabled != canGoBack { previous.isEnabled = canGoBack }
		if next.isEnabled != canGoForward { next.isEnabled = canGoForward }
	}

	@objc private func back() { scroll.move(by: -scroll.contentView.bounds.width * 0.8, animated: true) }
	@objc private func forward() { scroll.move(by: scroll.contentView.bounds.width * 0.8, animated: true) }
}

final class CardArrowButton: NSButton {
	private var hovered = false
	private var pressed = false
	private var hoverTracking: NSTrackingArea?

	override func updateTrackingAreas() {
		super.updateTrackingAreas()
		if let hoverTracking { removeTrackingArea(hoverTracking) }
		let tracking = NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect], owner: self)
		addTrackingArea(tracking)
		hoverTracking = tracking
	}

	override func mouseEntered(with event: NSEvent) { hovered = true; updateAppearance() }
	override func mouseExited(with event: NSEvent) { hovered = false; updateAppearance() }
	override func mouseDown(with event: NSEvent) {
		pressed = true
		updateAppearance()
		super.mouseDown(with: event)
		pressed = false
		updateAppearance()
	}

	override var isEnabled: Bool { didSet { if oldValue != isEnabled { updateAppearance() } } }

	private func updateAppearance() {
		wantsLayer = true
		layer?.cornerRadius = 5
		let color = NSColor.labelColor.withAlphaComponent(isEnabled ? (pressed ? 0.16 : hovered ? 0.08 : 0) : 0).cgColor
		if !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
			let transition = CABasicAnimation(keyPath: "backgroundColor")
			transition.fromValue = layer?.presentation()?.backgroundColor ?? layer?.backgroundColor
			transition.toValue = color
			transition.duration = 0.12
			layer?.add(transition, forKey: "hover")
		}
		layer?.backgroundColor = color
		contentTintColor = isEnabled ? .secondaryLabelColor : .tertiaryLabelColor
	}
}


/// Retargeting preserves velocity; Core Animation still renders every intermediate frame.
private struct CardScrollTransition {
	static let duration: Double = 0.22
	let start: CGFloat
	let target: CGFloat
	let began: Double
	let initialVelocity: CGFloat

	func velocity(at timestamp: Double) -> CGFloat {
		guard timestamp >= began, timestamp < began + Self.duration else { return 0 }
		let t = (timestamp - began) / Self.duration
		return (target - start) * 6 * t * (1 - t) / Self.duration
			+ initialVelocity * (1 - 4 * t + 3 * t * t)
	}
}
