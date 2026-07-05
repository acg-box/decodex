import Foundation

struct CodexAccount: Decodable, Identifiable, Equatable {
	let accountFingerprint: String
	let email: String?
	let selector: String
	let randomName: String?
	let randomNameKey: String?
	let randomNameOffset: Int?
	let status: String
	let selected: Bool
	let codexActive: Bool
	let disabled: Bool
	let refreshTokenPresent: Bool
	let accessTokenExpiresAtUnixEpoch: Int?
	let lastSelectedAtUnixEpoch: Int?
	let cooldownUntilUnixEpoch: Int?
	let note: String?
	let planType: String?
	let capacityMultiplier: Int?
	let recoveryAction: String?
	let refreshStatus: String?
	let checkedAtUnixEpoch: Int?
	let primaryWindowSeconds: Int?
	let primaryRemainingPercent: Int?
	let primaryResetsAtUnixEpoch: Int?
	let secondaryWindowSeconds: Int?
	let secondaryRemainingPercent: Int?
	let secondaryResetsAtUnixEpoch: Int?
	let creditsHasCredits: Bool?
	let creditsUnlimited: Bool?
	let creditsBalance: String?
	let rateLimitReachedType: String?
	let profileDisplayName: String?
	let profileUsername: String?
	let profileCheckedAtUnixEpoch: Int?
	let profileLifetimeTokens: Int?
	let profilePeakDailyTokens: Int?
	let profileLongestTaskSeconds: Int?
	let profileCurrentStreakDays: Int?
	let profileLongestStreakDays: Int?
	let profileDailyUsage: [AccountProfileDailyUsage]?
	let sevenDayUsedPercent: Int?
	let sevenDayDailyAveragePercent: Double?
	let usageRecords: [AccountUsageRecord]?

	enum CodingKeys: String, CodingKey {
		case accountFingerprint = "account_fingerprint"
		case email
		case selector
		case randomName = "random_name"
		case randomNameKey = "random_name_key"
		case randomNameOffset = "random_name_offset"
		case status
		case selected
		case codexActive = "codex_active"
		case disabled
		case refreshTokenPresent = "refresh_token_present"
		case accessTokenExpiresAtUnixEpoch = "access_token_expires_at_unix_epoch"
		case lastSelectedAtUnixEpoch = "last_selected_at_unix_epoch"
		case cooldownUntilUnixEpoch = "cooldown_until_unix_epoch"
		case note
		case planType = "plan_type"
		case capacityMultiplier = "capacity_multiplier"
		case recoveryAction = "recovery_action"
		case refreshStatus = "refresh_status"
		case checkedAtUnixEpoch = "checked_at_unix_epoch"
		case primaryWindowSeconds = "primary_window_seconds"
		case primaryRemainingPercent = "primary_remaining_percent"
		case primaryResetsAtUnixEpoch = "primary_resets_at_unix_epoch"
		case secondaryWindowSeconds = "secondary_window_seconds"
		case secondaryRemainingPercent = "secondary_remaining_percent"
		case secondaryResetsAtUnixEpoch = "secondary_resets_at_unix_epoch"
		case creditsHasCredits = "credits_has_credits"
		case creditsUnlimited = "credits_unlimited"
		case creditsBalance = "credits_balance"
		case rateLimitReachedType = "rate_limit_reached_type"
		case profileDisplayName = "profile_display_name"
		case profileUsername = "profile_username"
		case profileCheckedAtUnixEpoch = "profile_checked_at_unix_epoch"
		case profileLifetimeTokens = "profile_lifetime_tokens"
		case profilePeakDailyTokens = "profile_peak_daily_tokens"
		case profileLongestTaskSeconds = "profile_longest_task_seconds"
		case profileCurrentStreakDays = "profile_current_streak_days"
		case profileLongestStreakDays = "profile_longest_streak_days"
		case profileDailyUsage = "profile_daily_usage"
		case sevenDayUsedPercent = "seven_day_used_percent"
		case sevenDayDailyAveragePercent = "seven_day_daily_average_percent"
		case usageRecords = "usage_records"
	}
}

extension CodexAccount {
	var id: String {
		email ?? accountFingerprint
	}

	var displayName: String {
		email ?? accountFingerprint
	}

	var authIdentity: CodexAuthIdentity {
		CodexAuthIdentity(
			accountFingerprint: accountFingerprint,
			email: email,
			selector: selector
		)
	}

	func matchesSelector(_ value: String) -> Bool {
		let selector = value.trimmingCharacters(in: .whitespacesAndNewlines)
		return selector == email || selector == accountFingerprint || selector == self.selector
	}

	func withCodexActive(_ value: Bool) -> CodexAccount {
		CodexAccount(
			accountFingerprint: accountFingerprint,
			email: email,
			selector: selector,
			randomName: randomName,
			randomNameKey: randomNameKey,
			randomNameOffset: randomNameOffset,
			status: status,
			selected: selected,
			codexActive: value,
			disabled: disabled,
			refreshTokenPresent: refreshTokenPresent,
			accessTokenExpiresAtUnixEpoch: accessTokenExpiresAtUnixEpoch,
			lastSelectedAtUnixEpoch: lastSelectedAtUnixEpoch,
			cooldownUntilUnixEpoch: cooldownUntilUnixEpoch,
			note: note,
			planType: planType,
			capacityMultiplier: capacityMultiplier,
			recoveryAction: recoveryAction,
			refreshStatus: refreshStatus,
			checkedAtUnixEpoch: checkedAtUnixEpoch,
			primaryWindowSeconds: primaryWindowSeconds,
			primaryRemainingPercent: primaryRemainingPercent,
			primaryResetsAtUnixEpoch: primaryResetsAtUnixEpoch,
			secondaryWindowSeconds: secondaryWindowSeconds,
			secondaryRemainingPercent: secondaryRemainingPercent,
			secondaryResetsAtUnixEpoch: secondaryResetsAtUnixEpoch,
			creditsHasCredits: creditsHasCredits,
			creditsUnlimited: creditsUnlimited,
			creditsBalance: creditsBalance,
			rateLimitReachedType: rateLimitReachedType,
			profileDisplayName: profileDisplayName,
			profileUsername: profileUsername,
			profileCheckedAtUnixEpoch: profileCheckedAtUnixEpoch,
			profileLifetimeTokens: profileLifetimeTokens,
			profilePeakDailyTokens: profilePeakDailyTokens,
			profileLongestTaskSeconds: profileLongestTaskSeconds,
			profileCurrentStreakDays: profileCurrentStreakDays,
			profileLongestStreakDays: profileLongestStreakDays,
			profileDailyUsage: profileDailyUsage,
			sevenDayUsedPercent: sevenDayUsedPercent,
			sevenDayDailyAveragePercent: sevenDayDailyAveragePercent,
			usageRecords: usageRecords
		)
	}

	var needsLogin: Bool {
		recoveryActionKind == .login
	}

	var canUseInCodex: Bool {
		!disabled && recoveryActionKind != .login
	}

	var canRouteRuns: Bool {
		!disabled && recoveryActionKind != .login
	}

	var statusLabel: String {
		if isUsageLimited {
			return rawLimitStatusToken
		}
		if codexActive {
			return "codex_active"
		}
		if selected {
			return "selected"
		}
		if let action = rawRecoveryActionToken {
			return action
		}

		let token = status.trimmingCharacters(in: .whitespacesAndNewlines)
		return token.isEmpty ? "unknown" : token
	}

	var statusTone: AccountTone {
		if isUsageLimited {
			return .danger
		}
		if status == "probe_failed" {
			return .neutral
		}
		if codexActive {
			return .codexActive
		}
		if selected {
			return .selected
		}
		switch recoveryActionKind {
		case .login:
			return .danger
		case .refresh, .retryProbe:
			return .warning
		case .none:
			break
		}
		switch status {
		case "available": return .ready
		case "cooldown", "expired", "probe_failed": return .warning
		case "unusable", "disabled": return .danger
		default: return .neutral
		}
	}

	var recoveryActionKind: AccountRecoveryAction {
		if let recoveryAction = AccountRecoveryAction(rawValue: normalized(recoveryAction)) {
			return recoveryAction
		}
		if refreshTokenPresent == false {
			return .login
		}
		if normalized(refreshStatus) == "failed" {
			let noteText = normalized(note)
			return noteText.contains("401") || noteText.contains("unauthorized") ? .login : .retryProbe
		}
		switch normalized(status) {
		case "expired":
			return .refresh
		case "unusable":
			return .login
		case "probe_failed":
			return .retryProbe
		default:
			return .none
		}
	}

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

	private var rawRecoveryActionToken: String? {
		let token = recoveryAction?.trimmingCharacters(in: .whitespacesAndNewlines)
		return token?.isEmpty == false ? token : nil
	}

	private func normalized(_ value: String?) -> String {
		value?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() ?? ""
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
