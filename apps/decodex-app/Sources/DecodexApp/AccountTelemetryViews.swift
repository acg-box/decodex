import SwiftUI

struct AccountTelemetryMatrixView: View {
	let aggregate: AccountProfileAggregate?
	let usageEstimate: AccountUsageEstimate?
	let accounts: [CodexAccount]
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: AccountPanelLayout.telemetryRowSpacing) {
			if let aggregate {
				AccountProfileOverviewView(aggregate: aggregate)
			}

			if let usageEstimate {
				AccountPoolUsageEstimateView(estimate: usageEstimate, accounts: accounts)
			}
		}
		.padding(.horizontal, AccountPanelLayout.telemetryHorizontalPadding)
		.padding(.top, AccountPanelLayout.telemetryTopPadding)
		.padding(.bottom, AccountPanelLayout.telemetryBottomPadding)
		.frame(maxWidth: .infinity, alignment: .leading)
		.background {
			RoundedRectangle(cornerRadius: 9, style: .continuous)
				.fill(surfaceFill)
		}
		.clipShape(RoundedRectangle(cornerRadius: 9, style: .continuous))
		.id(colorScheme == .dark ? "telemetry-matrix-dark" : "telemetry-matrix-light")
	}

	private var surfaceFill: Color {
		colorScheme == .dark
			? Color(red: 0.08, green: 0.095, blue: 0.13).opacity(0.34)
			: Color(red: 0.9, green: 0.94, blue: 0.98).opacity(0.48)
	}
}
