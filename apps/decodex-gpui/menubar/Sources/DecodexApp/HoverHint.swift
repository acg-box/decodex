import AppKit
import SwiftUI

/// Passive hints use the same overlay preferences as GPUI menus and previews.
extension View {
	func hoverHint(_ text: String?) -> some View {
		background(HoverHintRegion(text: text))
	}
}

private struct HoverHintRegion: NSViewRepresentable {
	let text: String?
	func makeNSView(context: Context) -> HoverHintAnchor { HoverHintAnchor() }
	func updateNSView(_ view: HoverHintAnchor, context: Context) { view.text = text }
	static func dismantleNSView(_ view: HoverHintAnchor, coordinator: ()) {
		HoverHint.shared.hide(owner: view)
	}
}

final class HoverHintAnchor: NSView {
	var text: String? {
		didSet { if text != oldValue { HoverHint.shared.hide(owner: self) } }
	}
	private var tracking: NSTrackingArea?
	override func hitTest(_ point: NSPoint) -> NSView? { nil }
	override func updateTrackingAreas() {
		super.updateTrackingAreas()
		if let tracking { removeTrackingArea(tracking) }
		let next = NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self)
		addTrackingArea(next)
		tracking = next
	}
	override func mouseEntered(with event: NSEvent) { HoverHint.shared.schedule(owner: self, text: text) }
	override func mouseExited(with event: NSEvent) { HoverHint.shared.hide(owner: self) }
	override func viewDidMoveToWindow() {
		super.viewDidMoveToWindow()
		if window == nil { HoverHint.shared.hide(owner: self) }
	}
}

@MainActor
final class HoverHint {
	static let shared = HoverHint()
	static let delay: Duration = .milliseconds(500)
	private weak var owner: NSView?
	private var pending: Task<Void, Never>?
	private var panel: NSPanel?
	private var observers: [NSObjectProtocol] = []
	private var clicks: Any?

	private init() {
		for name in [NSApplication.didResignActiveNotification, NSWindow.willCloseNotification, NSWindow.didResignKeyNotification] {
			observers.append(NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) { [weak self] notification in
				let applicationResigned = notification.name == NSApplication.didResignActiveNotification
				let windowID = (notification.object as? NSWindow).map(ObjectIdentifier.init)
				MainActor.assumeIsolated {
					guard let self else { return }
					if applicationResigned || self.owner?.window.map(ObjectIdentifier.init) == windowID { self.dismiss() }
				}
			})
		}
		clicks = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown, .keyDown, .scrollWheel]) { [weak self] event in
			self?.dismiss()
			return event
		}
	}

	func schedule(owner: NSView, text: String?) {
		dismiss()
		guard let text, !text.isEmpty else { return }
		self.owner = owner
		pending = Task { [weak self, weak owner] in
			do { try await Task.sleep(for: Self.delay) } catch { return }
			guard let self, let owner, self.owner === owner, let window = owner.window,
				window.isVisible, (NSApp.isActive || window.isKeyWindow), !owner.isHiddenOrHasHiddenAncestor else { return }
			self.present(text, owner: owner, window: window)
		}
	}

	func hide(owner: NSView) {
		if self.owner === owner { dismiss() }
	}

	private func dismiss() {
		pending?.cancel()
		pending = nil
		owner = nil
		if let panel {
			panel.parent?.removeChildWindow(panel)
			panel.orderOut(nil)
		}
	}

	private func present(_ text: String, owner: NSView, window: NSWindow) {
		let label = NSTextField(wrappingLabelWithString: text)
		let paragraph = NSMutableParagraphStyle()
		paragraph.minimumLineHeight = 18
		paragraph.maximumLineHeight = 18
		label.attributedStringValue = NSAttributedString(string: text, attributes: [
			.font: NSFont.systemFont(ofSize: 12), .foregroundColor: NSColor.labelColor, .paragraphStyle: paragraph,
		])
		label.preferredMaxLayoutWidth = 344
		func measured(_ width: CGFloat) -> NSSize {
			label.cell?.cellSize(forBounds: NSRect(x: 0, y: 0, width: width, height: CGFloat.greatestFiniteMagnitude)) ?? .zero
		}
		let targetHeight = measured(344).height
		var lower: CGFloat = 1
		var width: CGFloat = 344
		while width - lower > 1 {
			let candidate = (lower + width) / 2
			if measured(candidate).height <= targetHeight { width = candidate }
			else { lower = candidate }
		}
		width = min(344, ceil(width) + 1)
		let size = NSSize(width: width + 16, height: ceil(measured(width).height) + 12)
		let foreground = NSView(frame: NSRect(origin: .zero, size: size))
		label.frame = NSRect(x: 8, y: 6, width: size.width - 16, height: size.height - 12)
		foreground.addSubview(label)
		foreground.wantsLayer = true
		foreground.layer?.cornerRadius = 10
		foreground.layer?.borderWidth = 0.5
		foreground.layer?.borderColor = NSColor.labelColor.withAlphaComponent(0.1).cgColor
		let defaults = UserDefaults.standard
		let material = defaults.string(forKey: "popoverMaterial") ?? defaults.string(forKey: "windowMaterial") ?? "liquid-glass"
		let backdrop: NSView
		if material == "liquid-glass", !NSWorkspace.shared.accessibilityDisplayShouldReduceTransparency {
			let glass = NSGlassEffectView(frame: foreground.bounds)
			let clear = (defaults.object(forKey: "DecodexPopoverGlassClear") as? Bool) ?? defaults.bool(forKey: "DecodexGlassClear")
			glass.style = clear ? NSGlassEffectView.Style.clear : NSGlassEffectView.Style.regular
			glass.cornerRadius = 10
			glass.contentView = foreground
			backdrop = glass
		} else {
			let thin = ThinMaterialView(frame: foreground.bounds)
			thin.cornerRadius = 10
			thin.contentView = foreground
			backdrop = thin
		}
		let hint = panel ?? NSPanel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
		panel = hint
		hint.title = "Decodex Hint"
		hint.isOpaque = false
		hint.backgroundColor = .clear
		hint.hasShadow = true
		hint.ignoresMouseEvents = true
		hint.hidesOnDeactivate = false
		hint.contentView = backdrop
		let anchor = window.convertToScreen(owner.convert(owner.bounds, to: nil))
		let screen = window.screen?.visibleFrame ?? anchor
		let candidates = [
			NSPoint(x: anchor.minX, y: anchor.minY - size.height - 8),
			NSPoint(x: anchor.minX, y: anchor.maxY + 8),
			NSPoint(x: anchor.maxX + 8, y: anchor.maxY - size.height),
			NSPoint(x: anchor.minX - size.width - 8, y: anchor.maxY - size.height),
		].map { point in
			NSPoint(
				x: min(max(screen.minX + 8, point.x), max(screen.minX + 8, screen.maxX - size.width - 8)),
				y: min(max(screen.minY + 8, point.y), max(screen.minY + 8, screen.maxY - size.height - 8))
			)
		}
		let origin = candidates.first { !NSRect(origin: $0, size: size).intersects(anchor.insetBy(dx: -8, dy: -8)) } ?? candidates[0]
		hint.setFrame(NSRect(origin: origin, size: size), display: true)
		window.addChildWindow(hint, ordered: .above)
		hint.orderFront(nil)
	}
}
