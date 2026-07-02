import Foundation

struct CodexFastModeResponse: Decodable, Equatable {
	let codexConfigPath: String
	let enabled: Bool

	enum CodingKeys: String, CodingKey {
		case codexConfigPath = "codex_config_path"
		case enabled
	}
}
