import AppKit
import QuartzCore
import SwiftUI

struct AccountRowContent {
	let id: String
	let content: AnyView
	let offset: CGFloat
	let isDragging: Bool
	var renderState: AccountRowRenderState? = nil
}

struct AccountRowRenderState: Equatable {
	let account: ResetCardAccountState
	let store: ObjectIdentifier
	let expanded: Bool
	let showsEmail: Bool
	let hovered: Bool
	let canDrag: Bool
	let dragging: Bool
	let colorScheme: ColorScheme
	let material: PanelCardMaterial
}

/// AppKit owns row placement; Core Animation moves and reveals the rendered rows.
/// SwiftUI only lays out the old and new content, never every animation frame.
struct AccountRows: NSViewRepresentable {
	let rows: [AccountRowContent]
	let reduceMotion: Bool
	let onHeightChange: (CGFloat) -> Void
	let onFramesChange: ([String: CGRect]) -> Void

	func makeNSView(context: Context) -> AccountRowsView { AccountRowsView() }
	func updateNSView(_ view: AccountRowsView, context: Context) {
		view.onHeightChange = onHeightChange
		view.onFramesChange = onFramesChange
		view.update(rows: rows, reduced: reduceMotion)
	}
	func sizeThatFits(_ proposal: ProposedViewSize, nsView: AccountRowsView, context: Context) -> CGSize? {
		CGSize(width: AccountPanelLayout.panelWidth, height: nsView.canvasHeight)
	}
}

@MainActor
final class AccountRowsView: NSView {
	static let animationKey = "decodex.account-layout"
	static let duration: TimeInterval = 0.24
	var onHeightChange: (CGFloat) -> Void = { _ in }
	var onFramesChange: ([String: CGRect]) -> Void = { _ in }
	private(set) var canvasHeight: CGFloat = 0
	private var targetHeight: CGFloat = 0
	private var rowViews: [String: AccountRowHostingView] = [:]
	private var frames: [String: CGRect] = [:]
	private var generation = 0
	override var isFlipped: Bool { true }

	func update(rows: [AccountRowContent], reduced: Bool) {
		let ids = Set(rows.map(\.id))
		let existingIDs = Set(rowViews.keys)
		for id in existingIDs.subtracting(ids) { rowViews.removeValue(forKey: id)?.removeFromSuperview() }
		let animate = !reduced && window != nil && existingIDs == ids && !existingIDs.isEmpty
		var nextFrames: [String: CGRect] = [:]
		var changes: [(AccountRowHostingView, CGRect, Bool)] = []
		var y: CGFloat = 1
		for row in rows {
			let view: AccountRowHostingView
			if let existing = rowViews[row.id] { view = existing }
			else {
				view = AccountRowHostingView(id: row.id, content: row.content)
				rowViews[row.id] = view
				addSubview(view)
			}
			let height = view.measure(row.content, renderState: row.renderState, retainClosingContent: animate)
			let base = CGRect(x: 1, y: y, width: AccountPanelLayout.panelWidth - 2, height: height)
			nextFrames[row.id] = base
			let placed = base.offsetBy(dx: 0, dy: row.offset)
			if view.frame != placed { changes.append((view, placed, row.isDragging)) }
			if row.isDragging { addSubview(view, positioned: .above, relativeTo: nil) }
			y += height + PanelSpacing.section
		}
		targetHeight = max(0, y - PanelSpacing.section + 1)
		guard !changes.isEmpty || frames != nextFrames else { return }
		generation += 1
		let current = generation
		// The transparent canvas accommodates both endpoints. Shrink it only after
		// the compositor finishes, so the moving bottom rows cannot be cut off.
		canvasHeight = animate ? max(canvasHeight, targetHeight) : targetHeight
		frames = nextFrames
		CATransaction.begin()
		CATransaction.setDisableActions(true)
		if animate {
			CATransaction.setCompletionBlock { [weak self] in
				MainActor.assumeIsolated { self?.finish(current) }
			}
		}
		for (view, frame, dragging) in changes {
			view.place(frame, animated: animate && !dragging, screen: window?.screen)
		}
		CATransaction.commit()
		report()
	}

	private func finish(_ completed: Int) {
		guard generation == completed else { return }
		for view in rowViews.values { view.finish() }
		canvasHeight = targetHeight
		report()
	}

	private func report() {
		let height = canvasHeight
		let frames = frames
		DispatchQueue.main.async { [weak self] in
			guard let self, self.canvasHeight == height, self.frames == frames else { return }
			self.onHeightChange(height)
			self.onFramesChange(frames)
		}
	}
}

@MainActor
private final class AccountRowHostingView: NSView {
	let host: NSHostingView<AnyView>
	private var pendingContent: AnyView?
	private var renderState: AccountRowRenderState?
	override var isFlipped: Bool { true }

	init(id: String, content: AnyView) {
		host = NSHostingView(rootView: content)
		host.sizingOptions = [.intrinsicContentSize]
		super.init(frame: .zero)
		wantsLayer = true
		layer?.cornerRadius = 16
		layer?.cornerCurve = .continuous
		setAccessibilityIdentifier("decodex.account.container.\(id)")
		addSubview(host)
	}
	required init?(coder: NSCoder) { fatalError("init(coder:) is unsupported") }

	func measure(_ content: AnyView, renderState: AccountRowRenderState?, retainClosingContent: Bool) -> CGFloat {
		let previous = host.rootView
		// Geometry reports must not rebuild every unrelated account's SwiftUI tree.
		if renderState == nil || self.renderState != renderState || pendingContent != nil { host.rootView = content }
		self.renderState = renderState
		host.frame.size.width = AccountPanelLayout.panelWidth - 2
		host.layoutSubtreeIfNeeded()
		let height = ceil(host.fittingSize.height)
		if retainClosingContent && height < host.frame.height {
			pendingContent = content
			host.rootView = previous
			host.layoutSubtreeIfNeeded()
		} else { pendingContent = nil }
		return height
	}

	func finish() {
		if let pendingContent {
			host.rootView = pendingContent
			self.pendingContent = nil
			host.frame = bounds
		}
		layer?.masksToBounds = false
	}

	func place(_ target: CGRect, animated: Bool, screen: NSScreen?) {
		let previous = layer?.presentation()
		let oldPosition = previous?.position ?? layer?.position ?? .zero
		let oldBounds = previous?.bounds ?? bounds
		layer?.removeAnimation(forKey: AccountRowsView.animationKey)
		host.layer?.removeAnimation(forKey: AccountRowsView.animationKey)
		frame = target
		host.frame = CGRect(x: 0, y: 0, width: target.width, height: pendingContent == nil ? target.height : max(host.frame.height, target.height))
		host.layoutSubtreeIfNeeded()
		guard animated, let layer else { self.layer?.masksToBounds = false; return }
		layer.masksToBounds = true
		let position = CABasicAnimation(keyPath: "position")
		position.fromValue = NSValue(point: oldPosition)
		position.toValue = NSValue(point: layer.position)
		let size = CABasicAnimation(keyPath: "bounds")
		size.fromValue = NSValue(rect: oldBounds)
		size.toValue = NSValue(rect: layer.bounds)
		let group = CAAnimationGroup()
		group.animations = [position, size]
		group.duration = AccountRowsView.duration
		group.timingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
		if let screen {
			let rate = Float(screen.maximumFramesPerSecond)
			group.preferredFrameRateRange = CAFrameRateRange(minimum: rate, maximum: rate, preferred: rate)
		}
		layer.add(group, forKey: AccountRowsView.animationKey)
	}

	// Model frames reach their destination before the compositor. Do not activate
	// a control at a position where it has not arrived yet.
	override func hitTest(_ point: NSPoint) -> NSView? {
		if layer?.animation(forKey: AccountRowsView.animationKey) != nil, let visible = layer?.presentation() {
			if abs(visible.frame.minY - frame.minY) > 0.5 || point.y - frame.minY > visible.bounds.height { return nil }
		}
		return super.hitTest(point)
	}
}
