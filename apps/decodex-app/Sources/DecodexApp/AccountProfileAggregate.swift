import Foundation

struct AccountProfileAggregate: Equatable {
	let accountCount: Int
	let lifetimeTokens: Int?
	let peakDailyTokens: Int?
	let longestTaskSeconds: Int?
	let currentStreakDays: Int?
	let longestStreakDays: Int?
	let dailyUsage: [AccountProfileDailyUsage]

	static func make(accounts: [CodexAccount]) -> AccountProfileAggregate? {
		var lifetimeTokens: Int?
		var peakFallbackTokens: Int?
		var longestTaskSeconds: Int?
		var currentStreakDays: Int?
		var longestStreakDays: Int?
		var usageByDate: [String: Int] = [:]

		for account in accounts {
			if let value = account.profileLifetimeTokens {
				lifetimeTokens = (lifetimeTokens ?? 0) + value
			}
			if let value = account.profilePeakDailyTokens {
				peakFallbackTokens = (peakFallbackTokens ?? 0) + value
			}
			if let value = account.profileLongestTaskSeconds {
				longestTaskSeconds = max(longestTaskSeconds ?? 0, value)
			}
			if let value = account.profileCurrentStreakDays {
				currentStreakDays = max(currentStreakDays ?? 0, value)
			}
			if let value = account.profileLongestStreakDays {
				longestStreakDays = max(longestStreakDays ?? 0, value)
			}
			for record in account.recentProfileDailyUsage {
				usageByDate[record.date, default: 0] += record.tokens
			}
		}

		let dailyUsage = usageByDate
			.map { AccountProfileDailyUsage(date: $0.key, tokens: $0.value) }
			.sorted { $0.date < $1.date }
		let peakDailyTokens = dailyUsage.map(\.tokens).max() ?? peakFallbackTokens
		let aggregate = AccountProfileAggregate(
			accountCount: accounts.count,
			lifetimeTokens: lifetimeTokens,
			peakDailyTokens: peakDailyTokens,
			longestTaskSeconds: longestTaskSeconds,
			currentStreakDays: currentStreakDays,
			longestStreakDays: longestStreakDays,
			dailyUsage: dailyUsage
		)

		return aggregate.hasProfileSummary ? aggregate : nil
	}

	var hasProfileSummary: Bool {
		lifetimeTokens != nil
			|| peakDailyTokens != nil
			|| longestTaskSeconds != nil
			|| currentStreakDays != nil
			|| longestStreakDays != nil
			|| dailyUsage.isEmpty == false
	}
}
