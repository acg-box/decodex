import Foundation

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
