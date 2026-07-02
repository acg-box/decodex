import Foundation

extension CodexAccount {
	var capacityWeight: Int {
		max(1, capacityMultiplier ?? Self.capacityMultiplier(for: planType))
	}

	var capacityLabel: String {
		"\(capacityWeight)x"
	}

	var currentCapacityLabel: String? {
		guard status == "available" || status == "usage_limited" else {
			return nil
		}
		guard checkedAtUnixEpoch != nil || hasUsageWindowData else {
			return nil
		}

		return capacityLabel
	}

	var hasUsageWindowData: Bool {
		primaryRemainingPercent != nil || secondaryRemainingPercent != nil
	}

	var hasProfileSummary: Bool {
		profileLifetimeTokens != nil
			|| profilePeakDailyTokens != nil
			|| profileLongestTaskSeconds != nil
			|| profileCurrentStreakDays != nil
			|| profileLongestStreakDays != nil
			|| recentProfileDailyUsage.isEmpty == false
	}

	var recentProfileDailyUsage: [AccountProfileDailyUsage] {
		profileDailyUsage ?? []
	}

	var profilePeakDailyTokensForDisplay: Int? {
		profilePeakDailyTokens ?? recentProfileDailyUsage.map(\.tokens).max()
	}

	var isUsageLimited: Bool {
		if let reached = rateLimitReachedType, !reached.isEmpty {
			return true
		}
		return status.contains("limit")
			|| primaryRemainingPercent == 0
			|| secondaryRemainingPercent == 0
	}

	func windowLabel(seconds: Int?) -> String {
		UsageWindowLabel.make(seconds: seconds)
	}

	func usageTone(remainingPercent: Int?) -> AccountTone {
		guard let remainingPercent else {
			return .neutral
		}
		if remainingPercent <= 10 {
			return .danger
		}
		if remainingPercent <= 25 {
			return .warning
		}
		return .ready
	}

	var rawLimitStatusToken: String {
		let reached = rateLimitReachedType?.trimmingCharacters(in: .whitespacesAndNewlines)
		if let reached, reached.isEmpty == false, reached != "none" {
			return reached
		}

		let token = status.trimmingCharacters(in: .whitespacesAndNewlines)
		return token.isEmpty || token == "available" ? "usage_limited" : token
	}

	private static func capacityMultiplier(for planType: String?) -> Int {
		guard let planType, !planType.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
			return 1
		}

		if planType.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() == "pro" {
			return 20
		}

		return 1
	}
}
