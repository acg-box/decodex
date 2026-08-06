import AppKit
import SwiftUI

private struct PanelWindowSizeReporter: NSViewRepresentable {
	let contentSize: CGSize
	let screenParametersRevision: Int
	let onVisibleFrameChange: (NSRect?) -> Void
	let onContentSizeChange: (CGSize) -> Void

	func makeNSView(context: Context) -> NSView {
		let view = PanelWindowSizingProbeView(frame: .zero)
		view.didMoveToWindow = { [weak coordinator = context.coordinator] view in
			PanelWindowAppearance.apply(to: view.window)
			coordinator?.retryReportAfterWindowAttachment(from: view)
		}
		return view
	}

	func updateNSView(_ nsView: NSView, context: Context) {
		PanelWindowAppearance.apply(to: nsView.window)
		context.coordinator.scheduleReport(
			from: nsView,
			contentSize: contentSize,
			screenParametersRevision: screenParametersRevision,
			onVisibleFrameChange: onVisibleFrameChange,
			onContentSizeChange: onContentSizeChange
		)
	}

	func makeCoordinator() -> Coordinator {
		Coordinator()
	}

	final class Coordinator {
		private var lastReportedScreenParametersRevision = -1
		private var pendingSize = CGSize.zero
		private var pendingScreenParametersRevision = 0
		private var pendingVisibleFrameCallback: (NSRect?) -> Void = { _ in }
		private var pendingContentSizeCallback: (CGSize) -> Void = { _ in }
		private var reportIsScheduled = false
		private var hasReportedVisibleFrame = false
		private var lastReportedVisibleFrame: NSRect?

		@MainActor
		func scheduleReport(
			from view: NSView,
			contentSize: CGSize,
			screenParametersRevision: Int,
			onVisibleFrameChange: @escaping (NSRect?) -> Void,
			onContentSizeChange: @escaping (CGSize) -> Void
		) {
			guard contentSize.width > 0, contentSize.height > 0 else {
				return
			}

			pendingSize = contentSize
			pendingScreenParametersRevision = screenParametersRevision
			pendingVisibleFrameCallback = onVisibleFrameChange
			pendingContentSizeCallback = onContentSizeChange
			guard reportIsScheduled == false else {
				return
			}

			reportIsScheduled = true
			DispatchQueue.main.async { [weak self, weak view] in
				guard let self, let view else {
					return
				}
				self.reportIsScheduled = false
				self.report(
					from: view,
					contentSize: self.pendingSize,
					screenParametersRevision: self.pendingScreenParametersRevision,
					onVisibleFrameChange: self.pendingVisibleFrameCallback,
					onContentSizeChange: self.pendingContentSizeCallback
				)
			}
		}

		@MainActor
		private func report(
			from view: NSView,
			contentSize: CGSize,
			screenParametersRevision: Int,
			onVisibleFrameChange: (NSRect?) -> Void,
			onContentSizeChange: (CGSize) -> Void
		) {
			guard let window = view.window else {
				return
			}

			if screenParametersRevision != lastReportedScreenParametersRevision {
				lastReportedScreenParametersRevision = screenParametersRevision
				hasReportedVisibleFrame = false
			}
			reportVisibleFrameIfNeeded(
				(window.screen ?? NSScreen.main)?.visibleFrame,
				onVisibleFrameChange: onVisibleFrameChange
			)
			onContentSizeChange(contentSize)
		}

		@MainActor
		func retryReportAfterWindowAttachment(from view: NSView) {
			guard pendingSize != .zero else {
				return
			}

			scheduleReport(
				from: view,
				contentSize: pendingSize,
				screenParametersRevision: pendingScreenParametersRevision,
				onVisibleFrameChange: pendingVisibleFrameCallback,
				onContentSizeChange: pendingContentSizeCallback
			)
		}

		@MainActor
		private func reportVisibleFrameIfNeeded(
			_ visibleFrame: NSRect?,
			onVisibleFrameChange: (NSRect?) -> Void
		) {
			guard hasReportedVisibleFrame == false
				|| Self.rectDiffers(visibleFrame, lastReportedVisibleFrame)
			else {
				return
			}

			hasReportedVisibleFrame = true
			lastReportedVisibleFrame = visibleFrame
			onVisibleFrameChange(visibleFrame)
		}

		private static func rectDiffers(_ lhs: NSRect?, _ rhs: NSRect?) -> Bool {
			switch (lhs, rhs) {
			case (.none, .none):
				return false
			case (.none, .some), (.some, .none):
				return true
			case (.some(let lhs), .some(let rhs)):
				return abs(lhs.minX - rhs.minX) > 0.5
					|| abs(lhs.minY - rhs.minY) > 0.5
					|| abs(lhs.width - rhs.width) > 0.5
					|| abs(lhs.height - rhs.height) > 0.5
			}
		}
	}
}

enum PanelWindowAppearance {
	@MainActor
	static func apply(to window: NSWindow?) {
		guard let window else {
			return
		}
		window.hasShadow = false
		window.isOpaque = false
		window.backgroundColor = .clear
		window.contentView?.wantsLayer = true
		window.contentView?.layer?.backgroundColor = NSColor.clear.cgColor
	}
}

private struct PanelWindowSizingModifier: ViewModifier {
	let onVisibleFrameChange: (NSRect?) -> Void
	let onContentSizeChange: (CGSize) -> Void
	@State private var contentSize = CGSize.zero
	@State private var screenParametersRevision = 0

	func body(content: Content) -> some View {
		content
			.fixedSize(horizontal: false, vertical: true)
			.background {
				GeometryReader { proxy in
					Color.clear.preference(key: PanelWindowContentSizeKey.self, value: proxy.size)
				}
			}
			.onPreferenceChange(PanelWindowContentSizeKey.self) { size in
				guard size != .zero else {
					return
				}
				contentSize = size
				onContentSizeChange(size)
			}
			.background {
				PanelWindowSizeReporter(
					contentSize: contentSize,
					screenParametersRevision: screenParametersRevision,
					onVisibleFrameChange: onVisibleFrameChange,
					onContentSizeChange: onContentSizeChange
				)
				.frame(width: 0, height: 0)
			}
			.onReceive(
				NotificationCenter.default.publisher(
					for: NSApplication.didChangeScreenParametersNotification
				)
			) { _ in
				screenParametersRevision &+= 1
			}
			.onReceive(
				NotificationCenter.default.publisher(
					for: NSWindow.didChangeScreenNotification
				)
			) { _ in
				screenParametersRevision &+= 1
			}
	}
}

extension View {
	func reportsPanelContentMetrics(
		onVisibleFrameChange: @escaping (NSRect?) -> Void = { _ in },
		onContentSizeChange: @escaping (CGSize) -> Void = { _ in }
	) -> some View {
		modifier(PanelWindowSizingModifier(
			onVisibleFrameChange: onVisibleFrameChange,
			onContentSizeChange: onContentSizeChange
		))
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
	static func roundedContentSize(for contentSize: CGSize) -> NSSize {
		NSSize(width: ceil(contentSize.width), height: ceil(contentSize.height))
	}
}
