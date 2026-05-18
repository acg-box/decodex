import Foundation

struct AccountListResponse: Decodable {
	let accountsPath: String
	let globalConfigPath: String
	let codexAuthPath: String
	let codexAuth: CodexAuthIdentity?
	let control: AccountControl
	let accounts: [CodexAccount]

	enum CodingKeys: String, CodingKey {
		case accountsPath = "accounts_path"
		case globalConfigPath = "global_config_path"
		case codexAuthPath = "codex_auth_path"
		case codexAuth = "codex_auth"
		case control
		case accounts
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

	var id: String {
		email ?? accountFingerprint
	}

	var displayName: String {
		email ?? accountFingerprint
	}

	var statusLabel: String {
		if codexActive {
			return "Codex active"
		}
		if selected {
			return "Decodex pinned"
		}

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
