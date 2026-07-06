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
	let resetCreditsAvailableCount: Int?
	let resetCreditsTotalEarnedCount: Int?
	let resetCreditsCheckedAtUnixEpoch: Int?
	let resetCredits: [AccountResetCredit]?
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
		case resetCreditsAvailableCount = "reset_credits_available_count"
		case resetCreditsTotalEarnedCount = "reset_credits_total_earned_count"
		case resetCreditsCheckedAtUnixEpoch = "reset_credits_checked_at_unix_epoch"
		case resetCredits = "reset_credits"
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

struct AccountResetCredit: Decodable, Equatable {
	let grantedAtUnixEpoch: Int?
	let expiresAtUnixEpoch: Int?
	let status: String?

	enum CodingKeys: String, CodingKey {
		case grantedAtUnixEpoch = "granted_at_unix_epoch"
		case expiresAtUnixEpoch = "expires_at_unix_epoch"
		case status
	}
}
