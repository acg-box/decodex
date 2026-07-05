import AppKit
import SwiftUI

private struct PanelWindowSizeReporter: NSViewRepresentable {
	let contentSize: CGSize

	func makeNSView(context: Context) -> NSView {
		let view = PanelWindowSizingProbeView(frame: .zero)
		view.didMoveToWindow = { [weak coordinator = context.coordinator] view in
			coordinator?.retryResizeAfterWindowAttachment(from: view)
		}
		return view
	}

	func updateNSView(_ nsView: NSView, context: Context) {
		context.coordinator.scheduleResize(from: nsView, contentSize: contentSize)
	}

	func makeCoordinator() -> Coordinator {
		Coordinator()
	}

	final class Coordinator {
		private var lastAppliedSize = NSSize.zero
		private var pendingSize = CGSize.zero
		private var resizeIsScheduled = false

		@MainActor
		func scheduleResize(from view: NSView, contentSize: CGSize) {
			guard contentSize.width > 0, contentSize.height > 0 else {
				return
			}

			pendingSize = contentSize
			guard resizeIsScheduled == false else {
				return
			}

			resizeIsScheduled = true
			DispatchQueue.main.async { [weak self, weak view] in
				guard let self, let view else {
					return
				}
				self.resizeIsScheduled = false
				self.resizeWindow(from: view, contentSize: self.pendingSize)
			}
		}

		@MainActor
		private func resizeWindow(from view: NSView, contentSize: CGSize) {
			guard let window = view.window else {
				return
			}

			let targetSize = PanelWindowSizingLayout.roundedContentSize(for: contentSize)
			guard Self.sizeDiffers(targetSize, lastAppliedSize)
				|| Self.sizeDiffers(targetSize, window.contentLayoutRect.size)
			else {
				return
			}

			let frame = PanelWindowSizingLayout.frame(
				forContentSize: targetSize,
				currentFrame: window.frame
			) { contentSize in
				window.frameRect(forContentRect: NSRect(origin: .zero, size: contentSize)).size
			} visibleFrame: {
				(window.screen ?? NSScreen.main)?.visibleFrame
			}
			window.setFrame(frame, display: true)
			lastAppliedSize = targetSize
		}

		@MainActor
		func retryResizeAfterWindowAttachment(from view: NSView) {
			guard pendingSize != .zero else {
				return
			}

			scheduleResize(from: view, contentSize: pendingSize)
		}

		private static func sizeDiffers(_ lhs: NSSize, _ rhs: NSSize) -> Bool {
			abs(lhs.width - rhs.width) > 0.5 || abs(lhs.height - rhs.height) > 0.5
		}
	}
}

private struct PanelWindowSizingModifier: ViewModifier {
	@State private var contentSize = CGSize.zero

	func body(content: Content) -> some View {
		content
			.fixedSize(horizontal: false, vertical: true)
			.background {
				GeometryReader { proxy in
					Color.clear.preference(key: PanelWindowContentSizeKey.self, value: proxy.size)
				}
			}
			.onPreferenceChange(PanelWindowContentSizeKey.self) { size in
				contentSize = size
			}
			.background {
				PanelWindowSizeReporter(contentSize: contentSize)
					.frame(width: 0, height: 0)
			}
	}
}

extension View {
	func sizesPanelWindowToContent() -> some View {
		modifier(PanelWindowSizingModifier())
	}
}

final class PanelWindowSizingProbeView: NSView {
	var didMoveToWindow: ((PanelWindowSizingProbeView) -> Void)?

	override func viewDidMoveToWindow() {
		super.viewDidMoveToWindow()
		didMoveToWindow?(self)
	}
}

struct PanelWindowContentSizeKey: PreferenceKey {
	static let defaultValue = CGSize.zero

	static func reduce(value: inout CGSize, nextValue: () -> CGSize) {
		let next = nextValue()
		guard next != .zero else {
			return
		}
		value = next
	}
}

enum PanelWindowSizingLayout {
	private static let placementMargin: CGFloat = 8

	static func roundedContentSize(for contentSize: CGSize) -> NSSize {
		NSSize(
			width: ceil(contentSize.width),
			height: ceil(contentSize.height)
		)
	}

	static func frame(
		forContentSize contentSize: NSSize,
		currentFrame: NSRect,
		frameSizeForContentSize: (NSSize) -> NSSize,
		visibleFrame: () -> NSRect?
	) -> NSRect {
		let frameSize = frameSizeForContentSize(contentSize)
		let proposedFrame = NSRect(
			x: currentFrame.midX - frameSize.width / 2,
			y: currentFrame.maxY - frameSize.height,
			width: frameSize.width,
			height: frameSize.height
		)
		guard let visibleFrame = visibleFrame() else {
			return proposedFrame
		}

		return NSRect(
			x: clamped(
				proposedFrame.origin.x,
				min: visibleFrame.minX + placementMargin,
				max: visibleFrame.maxX - frameSize.width - placementMargin
			),
			y: clamped(
				proposedFrame.origin.y,
				min: visibleFrame.minY + placementMargin,
				max: visibleFrame.maxY - frameSize.height - placementMargin
			),
			width: frameSize.width,
			height: frameSize.height
		)
	}

	private static func clamped(_ value: CGFloat, min: CGFloat, max: CGFloat) -> CGFloat {
		guard min <= max else {
			return min
		}

		return Swift.min(Swift.max(value, min), max)
	}
}
