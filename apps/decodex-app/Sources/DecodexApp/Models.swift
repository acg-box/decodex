import Foundation

struct AccountListResponse: Decodable {
	let accountsPath: String
	let globalConfigPath: String
	let codexAuthPath: String
	let codexAuth: CodexAuthIdentity?
	let control: AccountControl
	let accounts: [CodexAccount]
	let usageEstimate: AccountUsageEstimate?
	let usageProbeError: String?

	enum CodingKeys: String, CodingKey {
		case accountsPath = "accounts_path"
		case globalConfigPath = "global_config_path"
		case codexAuthPath = "codex_auth_path"
		case codexAuth = "codex_auth"
		case control
		case accounts
		case usageEstimate = "usage_estimate"
		case usageProbeError = "usage_probe_error"
	}
}

struct CodexAuthIdentity: Decodable {
	let accountFingerprint: String
	let email: String?
	let selector: String

	var displayName: String {
		email ?? accountFingerprint
	}

	enum CodingKeys: String, CodingKey {
		case accountFingerprint = "account_fingerprint"
		case email
		case selector
	}
}

struct CodexAuthUseResponse: Decodable {
	let codexAuthPath: String
	let account: CodexAuthIdentity

	enum CodingKeys: String, CodingKey {
		case codexAuthPath = "codex_auth_path"
		case account
	}
}

struct AccountControl: Decodable {
	let mode: String
	let accountSelector: String?

	enum CodingKeys: String, CodingKey {
		case mode
		case accountSelector = "account_selector"
	}
}

struct CodexFastModeResponse: Decodable, Equatable {
	let codexConfigPath: String
	let enabled: Bool

	enum CodingKeys: String, CodingKey {
		case codexConfigPath = "codex_config_path"
		case enabled
	}
}

struct AccountUsageEstimate: Decodable, Equatable {
	let windowDays: Int
	let accountCount: Int
	let accountEstimateCount: Int
	let totalCapacityPercent: Int
	let totalUsedPercent: Int
	let totalUsedOfCapacityPercent: Double
	let averageDailyUsedPercent: Double
	let averageDailyPoolPercent: Double

	enum CodingKeys: String, CodingKey {
		case windowDays = "window_days"
		case accountCount = "account_count"
		case accountEstimateCount = "account_estimate_count"
		case totalCapacityPercent = "total_capacity_percent"
		case totalUsedPercent = "total_used_percent"
		case totalUsedOfCapacityPercent = "total_used_of_capacity_percent"
		case averageDailyUsedPercent = "average_daily_used_percent"
		case averageDailyPoolPercent = "average_daily_pool_percent"
	}
}

struct AccountUsageRecord: Decodable, Identifiable, Equatable {
	let date: String
	let usedPercent: Int
	let checkedAtUnixEpoch: Int

	var id: String {
		"\(date)-\(checkedAtUnixEpoch)"
	}

	enum CodingKeys: String, CodingKey {
		case date
		case usedPercent = "used_percent"
		case checkedAtUnixEpoch = "checked_at_unix_epoch"
	}
}

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
	let sevenDayUsedPercent: Int?
	let sevenDayDailyAveragePercent: Double?
	let usageRecords: [AccountUsageRecord]?

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

	var needsLogin: Bool {
		status == "unusable" || status == "expired" || !refreshTokenPresent
	}

	var canUseInCodex: Bool {
		!disabled && !needsLogin
	}

	var canRouteRuns: Bool {
		!disabled && !needsLogin
	}

	var statusLabel: String {
		if isUsageLimited {
			return "Limited"
		}
		if codexActive {
			return "Codex active"
		}
		if selected {
			return "Runs routed"
		}

		switch status {
		case "available": return "Ready"
		case "usage_limited": return "Limited"
		case "probe_failed": return "-"
		case "expired": return "Refresh needed"
		case "disabled": return "Disabled"
		case "cooldown": return "Cooling"
		case "unusable": return "Needs login"
		default: return status.replacingOccurrences(of: "_", with: " ").capitalized
		}
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
		switch status {
		case "available": return .ready
		case "cooldown": return .warning
		case "expired", "unusable", "disabled": return .danger
		default: return .neutral
		}
	}

	var planLabel: String? {
		guard let planType, !planType.isEmpty else {
			return nil
		}

		return planType.replacingOccurrences(of: "_", with: " ").capitalized
	}

	var hasUsageWindowData: Bool {
		primaryRemainingPercent != nil || secondaryRemainingPercent != nil
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
			sevenDayUsedPercent: sevenDayUsedPercent,
			sevenDayDailyAveragePercent: sevenDayDailyAveragePercent,
			usageRecords: usageRecords
		)
	}

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
		case sevenDayUsedPercent = "seven_day_used_percent"
		case sevenDayDailyAveragePercent = "seven_day_daily_average_percent"
		case usageRecords = "usage_records"
	}
}

enum AccountTone {
	case codexActive
	case ready
	case selected
	case warning
	case danger
	case neutral
}

enum UsageWindowLabel {
	static func make(seconds: Int?) -> String {
		guard let seconds, seconds > 0 else {
			return "-"
		}

		if seconds == 18_000 {
			return "5h"
		}
		if seconds == 604_800 {
			return "7d"
		}
		if seconds % 86_400 == 0 {
			return days(seconds / 86_400)
		}
		if seconds % 3_600 == 0 {
			return "\(seconds / 3_600)h"
		}

		return "-"
	}

	static func days(_ value: Int) -> String {
		"\(value)d"
	}
}
