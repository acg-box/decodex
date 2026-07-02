import Foundation

enum DecodexAppBridgeError: LocalizedError {
	case helperMissing(String)
	case launchFailed(String)
	case commandFailed(Int32, String)
	case invalidResponse(String)

	var errorDescription: String? {
		switch self {
		case .helperMissing(let message):
			return message
		case .launchFailed(let message):
			return message
		case .commandFailed(let code, let message):
			return "Decodex App bridge command failed with status \(code): \(message)"
		case .invalidResponse(let message):
			return "Invalid Decodex App bridge response: \(message)"
		}
	}
}
