import Foundation

extension CodexAccount {
	var id: String {
		email ?? accountFingerprint
	}

	var displayName: String {
		email ?? accountFingerprint
	}

	var authIdentity: CodexAuthIdentity {
		CodexAuthIdentity(
			accountFingerprint: accountFingerprint,
			email: email,
			selector: selector
		)
	}

	func matchesSelector(_ value: String) -> Bool {
		let selector = value.trimmingCharacters(in: .whitespacesAndNewlines)
		return selector == email || selector == accountFingerprint || selector == self.selector
	}
}
