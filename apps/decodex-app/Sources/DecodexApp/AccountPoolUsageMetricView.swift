import SwiftUI

struct AccountPoolUsageMetricView: View {
	let title: String
	let value: String
	let tint: Color
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: 3) {
			Text(title)
				.font(PanelFont.usageLabel)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.82))
				.lineLimit(1)

			Text(value)
				.font(PanelFont.usageValue)
				.foregroundStyle(tint.opacity(colorScheme == .dark ? 0.94 : 0.78))
				.monospacedDigit()
				.lineLimit(1)
				.minimumScaleFactor(0.72)
		}
		.lineLimit(1)
	}
}
