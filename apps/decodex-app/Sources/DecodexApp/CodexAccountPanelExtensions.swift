import SwiftUI

extension CodexAccount {
	var randomNameSeed: String {
		if accountFingerprint.isEmpty == false {
			return accountFingerprint
		}
		if let email, email.isEmpty == false {
			return email
		}
		if let planType, planType.isEmpty == false {
			return planType
		}

		return "account"
	}

	func panelDisplayName(emailsHidden: Bool) -> String {
		if emailsHidden {
			if let randomName = randomName?.trimmingCharacters(in: .whitespacesAndNewlines),
				randomName.isEmpty == false
			{
				return randomName
			}

			return AccountDisplay.alias(for: self)
		}

		return AccountDisplay.compactEmail(displayName)
	}

	func statusDisplayColor(colorScheme: ColorScheme) -> Color {
		switch statusTone {
		case .codexActive:
			return PanelPalette.codexAccent(colorScheme)
		case .ready:
			return PanelPalette.secondaryText(colorScheme)
		case .selected:
			return PanelPalette.routeAccent(colorScheme)
		case .warning:
			return PanelPalette.warning(colorScheme)
		case .danger:
			return PanelPalette.destructive(colorScheme)
		case .neutral:
			return PanelPalette.secondaryText(colorScheme)
		}
	}

	var hasPrimaryUsageData: Bool {
		primaryRemainingPercent != nil || primaryWindowSeconds != nil || primaryResetsAtUnixEpoch != nil
	}

	var hasSecondaryUsageData: Bool {
		secondaryRemainingPercent != nil || secondaryWindowSeconds != nil || secondaryResetsAtUnixEpoch != nil
	}

	var hasUsageSummary: Bool {
		hasUsageWindowSummary || hasProfileSummary || hasResetCreditsSummary
	}

	var hasUsageWindowSummary: Bool {
		hasPrimaryUsageData || hasSecondaryUsageData
	}

	var availableResetCredits: [AccountResetCredit] {
		(resetCredits ?? [])
			.filter { credit in
				let status = credit.status?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
				return status == nil || status == "" || status == "available"
			}
			.sorted {
				($0.expiresAtUnixEpoch ?? Int.max) < ($1.expiresAtUnixEpoch ?? Int.max)
			}
	}

	var visibleResetCreditCount: Int? {
		if let resetCreditsAvailableCount {
			return resetCreditsAvailableCount
		}
		let count = availableResetCredits.count
		return count > 0 ? count : nil
	}

	var hasResetCreditsSummary: Bool {
		visibleResetCreditCount.map { $0 > 0 } ?? false
	}

	var recentUsageRecords: [AccountUsageRecord] {
		usageRecords ?? []
	}

	func sevenDayAveragePercent(forWindowSeconds seconds: Int?) -> Double? {
		guard seconds == 604_800 else {
			return nil
		}

		return sevenDayDailyAveragePercent
	}

	var compactHealthLabel: String? {
		if isUsageLimited {
			return compactLimitStatusToken
		}

		if let token = recoveryAction?.trimmingCharacters(in: .whitespacesAndNewlines),
			token.isEmpty == false
		{
			return token
		}

		let label = status.trimmingCharacters(in: .whitespacesAndNewlines)
		return label.isEmpty || label == "available" ? nil : label
	}

	private var compactLimitStatusToken: String {
		let reached = rateLimitReachedType?.trimmingCharacters(in: .whitespacesAndNewlines)
		if let reached, reached.isEmpty == false, reached != "none" {
			return reached
		}

		let token = status.trimmingCharacters(in: .whitespacesAndNewlines)
		return token.isEmpty || token == "available" ? "usage_limited" : token
	}
}
