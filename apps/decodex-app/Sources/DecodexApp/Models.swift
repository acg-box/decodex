import Foundation

struct AccountListResponse: Decodable {
	let accountsPath: String
	let globalConfigPath: String
	let control: AccountControl
	let accounts: [CodexAccount]

	enum CodingKeys: String, CodingKey {
		case accountsPath = "accounts_path"
		case globalConfigPath = "global_config_path"
		case control
		case accounts
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
	let disabled: Bool
	let refreshTokenPresent: Bool
	let accessTokenExpiresAtUnixEpoch: Int?
	let lastSelectedAtUnixEpoch: Int?
	let cooldownUntilUnixEpoch: Int?
	let note: String?

	var id: String {
		email ?? accountFingerprint
	}

	var displayName: String {
		email ?? accountFingerprint
	}

	var statusLabel: String {
		switch status {
		case "available": return "Ready"
		case "expired": return "Refresh needed"
		case "disabled": return "Disabled"
		case "cooldown": return "Cooling"
		case "unusable": return "Needs login"
		default: return status.replacingOccurrences(of: "_", with: " ").capitalized
		}
	}

	var statusTone: AccountTone {
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

	enum CodingKeys: String, CodingKey {
		case accountFingerprint = "account_fingerprint"
		case email
		case selector
		case status
		case selected
		case disabled
		case refreshTokenPresent = "refresh_token_present"
		case accessTokenExpiresAtUnixEpoch = "access_token_expires_at_unix_epoch"
		case lastSelectedAtUnixEpoch = "last_selected_at_unix_epoch"
		case cooldownUntilUnixEpoch = "cooldown_until_unix_epoch"
		case note
	}
}

enum AccountTone {
	case ready
	case selected
	case warning
	case danger
	case neutral
}
