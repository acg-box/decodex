import Foundation

struct AppBridgeRequest: Encodable, Sendable {
	let operation: String
	let selector: String?
	let authJsonPath: String?
	let codexBin: String?
	let keepTempHome: Bool?
	let includeUsage: Bool?
	let forceRefresh: Bool?
	let enabled: Bool?

	enum CodingKeys: String, CodingKey {
		case operation
		case selector
		case authJsonPath = "auth_json_path"
		case codexBin = "codex_bin"
		case keepTempHome = "keep_temp_home"
		case includeUsage = "include_usage"
		case forceRefresh = "force_refresh"
		case enabled
	}

	static func accountList(forceRefresh: Bool = false) -> AppBridgeRequest {
		AppBridgeRequest(
			operation: "account_list",
			includeUsage: true,
			forceRefresh: forceRefresh
		)
	}

	static let accountClear = AppBridgeRequest(operation: "account_clear", includeUsage: true)

	static func accountUse(selector: String) -> AppBridgeRequest {
		AppBridgeRequest(operation: "account_use", selector: selector)
	}

	static func accountSelect(selector: String) -> AppBridgeRequest {
		AppBridgeRequest(operation: "account_select", selector: selector, includeUsage: true)
	}

	static func accountLogout(selector: String) -> AppBridgeRequest {
		AppBridgeRequest(operation: "account_logout", selector: selector, includeUsage: true)
	}

	static func accountLogin() -> AppBridgeRequest {
		AppBridgeRequest(operation: "account_login", includeUsage: true)
	}

	static let codexFastModeStatus = AppBridgeRequest(operation: "codex_fast_mode_status")

	static func codexFastModeSet(enabled: Bool) -> AppBridgeRequest {
		AppBridgeRequest(operation: "codex_fast_mode_set", enabled: enabled)
	}

	private init(
		operation: String,
		selector: String? = nil,
		authJsonPath: String? = nil,
		codexBin: String? = nil,
		keepTempHome: Bool? = nil,
		includeUsage: Bool? = nil,
		forceRefresh: Bool? = nil,
		enabled: Bool? = nil
	) {
		self.operation = operation
		self.selector = selector
		self.authJsonPath = authJsonPath
		self.codexBin = codexBin
		self.keepTempHome = keepTempHome
		self.includeUsage = includeUsage
		self.forceRefresh = forceRefresh
		self.enabled = enabled
	}
}
