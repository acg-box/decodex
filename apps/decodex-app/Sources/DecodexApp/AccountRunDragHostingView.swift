import AppKit
import SwiftUI

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
