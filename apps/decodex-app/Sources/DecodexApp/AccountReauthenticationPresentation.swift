import Foundation

enum AccountReauthenticationPhase: Equatable {
	case resolvingCodex
	case openingBrowser
	case waitingForBrowser
	case installing
	case failed(String)
	case cancellationFailed(String)
}

struct AccountReauthenticationPresentation: Identifiable, Equatable {
	let accountID: String
	let accountLabel: String
	let sessionID: String
	let authority: ResetCardAuthority?
	let phase: AccountReauthenticationPhase

	var id: String {
		sessionID
	}

	var statusText: String {
		switch phase {
		case .resolvingCodex:
			return "Finding Codex"
		case .openingBrowser:
			return "Opening browser sign-in"
		case .waitingForBrowser:
			return "Waiting for browser sign-in"
		case .installing:
			return "Saving this login"
		case .failed:
			return "Login failed"
		case .cancellationFailed:
			return "Could not cancel login"
		}
	}

	var failureText: String? {
		switch phase {
		case .failed(let message), .cancellationFailed(let message):
			return message
		case .resolvingCodex, .openingBrowser, .waitingForBrowser, .installing:
			return nil
		}
	}

	var canCloseWithoutCancellation: Bool {
		if case .failed = phase {
			return true
		}
		return false
	}

	var canRequestCancellation: Bool {
		if case .installing = phase {
			return false
		}
		return canCloseWithoutCancellation == false
	}
}
