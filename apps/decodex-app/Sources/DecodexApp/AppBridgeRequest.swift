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

	static func accountUse(
		selector: String,
		authJsonPath: String? = nil
	) -> AppBridgeRequest {
		AppBridgeRequest(
			operation: "account_use",
			selector: selector,
			authJsonPath: authJsonPath
		)
	}

	static func accountSelect(selector: String) -> AppBridgeRequest {
		AppBridgeRequest(operation: "account_select", selector: selector, includeUsage: true)
	}

	static func accountLogout(selector: String) -> AppBridgeRequest {
		AppBridgeRequest(operation: "account_logout", selector: selector, includeUsage: true)
	}

	static func accountLogin(codexBin: String) -> AppBridgeRequest {
		AppBridgeRequest(operation: "account_login", codexBin: codexBin, includeUsage: true)
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

extension AppBridgeRequest {
	func serverRoute() throws -> ServerRoute? {
		switch operation {
		case "account_list":
			let suffix = forceRefresh == true ? "?refresh=1" : ""

			return ServerRoute(method: "GET", path: "api/accounts\(suffix)", body: nil)
		case "account_select":
			return try jsonPost("api/accounts/select")
		case "account_clear":
			return try jsonPost("api/accounts/clear")
		case "account_logout":
			return try jsonPost("api/accounts/logout")
		case "account_import":
			return try jsonPost("api/accounts/import")
		case "account_use":
			return try jsonPost("api/accounts/use")
		default:
			return nil
		}
	}

	private func jsonPost(_ path: String) throws -> ServerRoute {
		ServerRoute(method: "POST", path: path, body: try JSONEncoder().encode(self))
	}
}

extension AppBridgeRequest {
	var requiresHelper: Bool {
		switch operation {
		case
			"account_import",
			"account_login",
			"codex_fast_mode_status",
			"codex_fast_mode_set":
			return true
		default:
			return false
		}
	}
}
