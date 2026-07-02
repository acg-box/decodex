import Foundation

enum AccountTone {
	case codexActive
	case ready
	case selected
	case warning
	case danger
	case neutral
}

enum AccountRecoveryAction: String {
	case none
	case refresh
	case login
	case retryProbe = "retry_probe"
}

enum UsageWindowLabel {
	static func make(seconds: Int?) -> String {
		guard let seconds, seconds > 0 else {
			return "-"
		}

		if seconds == 18_000 {
			return "5h"
		}
		if seconds == 604_800 {
			return "7d"
		}
		if seconds % 86_400 == 0 {
			return days(seconds / 86_400)
		}
		if seconds % 3_600 == 0 {
			return "\(seconds / 3_600)h"
		}

		return "-"
	}

	static func days(_ value: Int) -> String {
		"\(value)d"
	}
}
