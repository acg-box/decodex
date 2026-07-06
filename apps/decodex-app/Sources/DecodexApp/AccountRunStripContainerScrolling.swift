import AppKit

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
