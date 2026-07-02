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
