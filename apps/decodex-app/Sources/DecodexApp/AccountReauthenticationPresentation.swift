import Foundation

enum AccountLoginMode: Equatable {
	case enrollment
	case reauthentication

	var title: String {
		switch self {
		case .enrollment:
			return "Add account"
		case .reauthentication:
			return "Refresh login"
		}
	}

	var accessibilityLabel: String {
		switch self {
		case .enrollment:
			return "Add account"
		case .reauthentication:
			return "Refresh login"
		}
	}

	var cancelActionLabel: String {
		switch self {
		case .enrollment:
			return "Cancel adding account"
		case .reauthentication:
			return "Cancel login"
		}
	}

	var closeActionLabel: String {
		switch self {
		case .enrollment:
			return "Close add account"
		case .reauthentication:
			return "Close login"
		}
	}

	var installingLabel: String {
		switch self {
		case .enrollment:
			return "Adding account"
		case .reauthentication:
			return "Saving login"
		}
	}
}

enum AccountReauthenticationPhase: Equatable {
	case selectingMethod
	case resolvingCodex
	case requestingCode
	case openingBrowser
	case waitingForBrowser
	case installing
	case failed(String)
	case cancellationFailed(String)
}

struct AccountReauthenticationPresentation: Identifiable, Equatable {
	let mode: AccountLoginMode
	let accountID: String
	let accountLabel: String
	let sessionID: String
	let authority: ResetCardAuthority?
	let phase: AccountReauthenticationPhase
	let loginMethod: AccountLoginMethod?
	let prompt: AccountReauthenticationPrompt?

	var id: String {
		sessionID
	}

	var statusText: String {
		switch phase {
		case .selectingMethod:
			return "Choose a sign-in method"
		case .resolvingCodex:
			return "Finding Codex"
		case .requestingCode:
			return "Requesting a one-time code"
		case .openingBrowser:
			return "Opening browser sign-in"
		case .waitingForBrowser:
			return "Waiting for browser sign-in"
		case .installing:
			return mode.installingLabel
		case .failed:
			return "Login failed"
		case .cancellationFailed:
			return "Could not cancel login"
		}
	}

	var title: String {
		mode.title
	}

	var headerAccountLabel: String? {
		guard mode == .reauthentication else {
			return nil
		}
		let label = accountLabel.trimmingCharacters(in: .whitespacesAndNewlines)
		guard label.isEmpty == false, label != title else {
			return nil
		}
		return label
	}

	var accessibilityLabel: String {
		mode.accessibilityLabel
	}

	var cancelActionLabel: String {
		mode.cancelActionLabel
	}

	var closeActionLabel: String {
		mode.closeActionLabel
	}

	var failureText: String? {
		switch phase {
		case .failed(let message), .cancellationFailed(let message):
			return message
		case .selectingMethod, .resolvingCodex, .requestingCode, .openingBrowser,
			.waitingForBrowser, .installing:
			return nil
		}
	}

	var isSelectingMethod: Bool {
		phase == .selectingMethod
	}

	var showsStatusText: Bool {
		isSelectingMethod == false
	}

	var showsProgress: Bool {
		guard prompt == nil else {
			return false
		}
		switch phase {
		case .selectingMethod, .failed, .cancellationFailed:
			return false
		case .resolvingCodex, .requestingCode, .openingBrowser,
			.waitingForBrowser, .installing:
			return true
		}
	}

	var canCloseWithoutCancellation: Bool {
		if case .failed = phase {
			return true
		}
		if case .selectingMethod = phase {
			return true
		}
		return false
	}

	var canRequestCancellation: Bool {
		if case .selectingMethod = phase {
			return false
		}
		if case .installing = phase {
			return false
		}
		return canCloseWithoutCancellation == false
	}
}
