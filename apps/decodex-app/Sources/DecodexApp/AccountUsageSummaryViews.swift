import Foundation
import SwiftUI

struct AccountUsageSummaryView: View {
	let account: CodexAccount

	var body: some View {
		TimelineView(.periodic(from: Date(), by: 30)) { timeline in
			VStack(spacing: 5) {
				if account.hasProfileSummary {
					AccountProfileSummaryView(account: account)
				}

				if account.hasPrimaryUsageData {
					AccountUsageMeterView(
						label: account.windowLabel(seconds: account.primaryWindowSeconds),
						remainingPercent: account.primaryRemainingPercent,
						resetAtUnixEpoch: account.primaryResetsAtUnixEpoch,
						dailyAveragePercent: account.sevenDayAveragePercent(
							forWindowSeconds: account.primaryWindowSeconds
						),
						tone: account.usageTone(remainingPercent: account.primaryRemainingPercent),
						currentTime: timeline.date
					)
				}

				if account.hasSecondaryUsageData {
					AccountUsageMeterView(
						label: account.windowLabel(seconds: account.secondaryWindowSeconds),
						remainingPercent: account.secondaryRemainingPercent,
						resetAtUnixEpoch: account.secondaryResetsAtUnixEpoch,
						dailyAveragePercent: account.sevenDayAveragePercent(
							forWindowSeconds: account.secondaryWindowSeconds
						),
						tone: account.usageTone(remainingPercent: account.secondaryRemainingPercent),
						currentTime: timeline.date
					)
				}
			}
			.frame(maxWidth: .infinity)
			.padding(.horizontal, 1)
			.padding(.vertical, 1)
		}
	}
}
