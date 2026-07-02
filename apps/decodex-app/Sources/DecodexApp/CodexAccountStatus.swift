import Foundation

extension CodexAccount {
	var needsLogin: Bool {
		recoveryActionKind == .login
	}

	var canUseInCodex: Bool {
		!disabled && recoveryActionKind != .login
	}

	var canRouteRuns: Bool {
		!disabled && recoveryActionKind != .login
	}

	var statusLabel: String {
		if isUsageLimited {
			return rawLimitStatusToken
		}
		if codexActive {
			return "codex_active"
		}
		if selected {
			return "selected"
		}
		if let action = rawRecoveryActionToken {
			return action
		}

		let token = status.trimmingCharacters(in: .whitespacesAndNewlines)
		return token.isEmpty ? "unknown" : token
	}

	var statusTone: AccountTone {
		if isUsageLimited {
			return .danger
		}
		if status == "probe_failed" {
			return .neutral
		}
		if codexActive {
			return .codexActive
		}
		if selected {
			return .selected
		}
		switch recoveryActionKind {
		case .login:
			return .danger
		case .refresh, .retryProbe:
			return .warning
		case .none:
			break
		}
		switch status {
		case "available": return .ready
		case "cooldown", "expired", "probe_failed": return .warning
		case "unusable", "disabled": return .danger
		default: return .neutral
		}
	}

	var recoveryActionKind: AccountRecoveryAction {
		if let recoveryAction = AccountRecoveryAction(rawValue: normalized(recoveryAction)) {
			return recoveryAction
		}
		if refreshTokenPresent == false {
			return .login
		}
		if normalized(refreshStatus) == "failed" {
			let noteText = normalized(note)
			return noteText.contains("401") || noteText.contains("unauthorized") ? .login : .retryProbe
		}
		switch normalized(status) {
		case "expired":
			return .refresh
		case "unusable":
			return .login
		case "probe_failed":
			return .retryProbe
		default:
			return .none
		}
	}

	private var rawRecoveryActionToken: String? {
		let token = recoveryAction?.trimmingCharacters(in: .whitespacesAndNewlines)
		return token?.isEmpty == false ? token : nil
	}

	private func normalized(_ value: String?) -> String {
		value?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() ?? ""
	}
}
