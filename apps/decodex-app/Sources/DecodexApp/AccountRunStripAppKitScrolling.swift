import AppKit
import Foundation
import SwiftUI

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
	var allowsPointerPanning = true {
		didSet {
			guard allowsPointerPanning != oldValue else {
				return
			}

			dragStartPoint = nil
			isDraggingContent = false
			window?.invalidateCursorRects(for: self)
		}
	}
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
		guard allowsPointerPanning, let dragScrollView else {
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
