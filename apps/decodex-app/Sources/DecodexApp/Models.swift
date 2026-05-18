import Foundation

struct AccountListResponse: Decodable {
	let accountsPath: String
	let globalConfigPath: String
	let codexAuthPath: String
	let codexAuth: CodexAuthIdentity?
	let control: AccountControl
	let accounts: [CodexAccount]
	let usageProbeError: String?

	enum CodingKeys: String, CodingKey {
		case accountsPath = "accounts_path"
		case globalConfigPath = "global_config_path"
		case codexAuthPath = "codex_auth_path"
		case codexAuth = "codex_auth"
		case control
		case accounts
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

struct CodexAccount: Decodable, Identifiable, Equatable {
	let accountFingerprint: String
	let email: String?
	let selector: String
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

	var id: String {
		email ?? accountFingerprint
	}

	var displayName: String {
		email ?? accountFingerprint
	}

	var statusLabel: String {
		if isUsageLimited {
			return "Limited"
		}
		if codexActive {
			return "Codex active"
		}
		if selected {
			return "Decodex pinned"
		}

		switch status {
		case "available": return "Ready"
		case "usage_limited": return "Limited"
		case "probe_failed": return "Usage unknown"
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
			return .warning
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
		switch seconds {
		case 18_000: return "5h"
		case 604_800: return "7d"
		case let value?:
			let hours = value / 3_600
			if hours > 0 && value % 3_600 == 0 {
				return "\(hours)h"
			}
			let days = value / 86_400
			if days > 0 && value % 86_400 == 0 {
				return "\(days)d"
			}
			return "window"
		case nil:
			return "window"
		}
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

	enum CodingKeys: String, CodingKey {
		case accountFingerprint = "account_fingerprint"
		case email
		case selector
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
