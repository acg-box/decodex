import Foundation

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
