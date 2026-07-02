import Foundation
import SwiftUI

struct AccountPoolUsageEstimateView: View {
	let estimate: AccountUsageEstimate
	let accounts: [CodexAccount]
	@Environment(\.colorScheme) var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 3) {
			HStack(spacing: 5) {
				ForEach(Array(metrics.enumerated()), id: \.offset) { index, metric in
					AccountPoolUsageMetricView(
						title: metric.title,
						value: metric.value,
						tint: metric.tint
					)

					if index < metrics.count - 1 {
						Spacer(minLength: 3)
					}
				}
			}
			.frame(height: 16)

			if estimate.accountEstimateCount < estimate.accountCount {
				Text("\(estimate.accountEstimateCount)/\(estimate.accountCount) accounts measured")
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.72))
					.lineLimit(1)
			}
		}
			.frame(maxWidth: .infinity, alignment: .leading)
			.accessibilityLabel(accessibilityLabel)
	}
}
