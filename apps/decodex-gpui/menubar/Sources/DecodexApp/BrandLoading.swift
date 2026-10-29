import SwiftUI

/// Reuse the packaged Decodex mark. Match the main window's outward pixel motion.
struct BrandLoading: View {
	var size: CGFloat = 18
	@Environment(\.accessibilityReduceMotion) private var reduceMotion
	@State private var started = Date()

	var body: some View {
		TimelineView(.animation(minimumInterval: 1.0 / 30, paused: reduceMotion)) { context in
			let elapsed = reduceMotion ? 1.4 : context.date.timeIntervalSince(started)
			ZStack(alignment: .topLeading) {
				Image(nsImage: AppAssets.statusBarIcon)
					.resizable()
					.foregroundStyle(.secondary)
					.frame(width: size, height: size)
				Canvas { canvas, _ in
					for index in 0..<10 {
						let phase = (elapsed / 2.8 + Double(index) / 10).truncatingRemainder(dividingBy: 1)
						let lane = Double(index % 4)
						let seed = Double((index * 7) % 11) / 10
						let x = 11.3 + lane * 1.77 + phase * (7 + seed * 3)
						let y = 6.2 + lane * 1.77 - phase * (5 + seed * 3)
							+ sin(phase * .pi) * 0.7 * (seed - 0.5)
						let alpha = Self.smooth(phase / 0.12)
							* (1 - Self.smooth((phase - 0.55) / 0.45)) * (0.6 + seed * 0.35)
						let scale = size / 24
						canvas.fill(
							Path(CGRect(x: x * scale, y: y * scale, width: 1.5 * scale, height: 1.5 * scale)),
							with: .color(.secondary.opacity(alpha))
						)
					}
				}
			}
			.frame(width: size * 4 / 3, height: size)
		}
		.accessibilityLabel("Loading")
	}

	private static func smooth(_ value: Double) -> Double {
		let value = min(1, max(0, value))
		return value * value * (3 - 2 * value)
	}
}
