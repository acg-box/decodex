import Foundation

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
