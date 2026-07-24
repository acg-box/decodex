import Foundation

struct AccountNotice: Equatable, Identifiable, Sendable {
	enum Tone: Equatable, Sendable {
		case success
		case information
		case error
	}

	enum Scope: Equatable, Sendable {
		case general
		case signIn
	}

	enum Source: Equatable, Sendable {
		case accountAction
		case accountRefresh
		case fastMode
		case resetCredit
		case signIn
	}

	let id: UUID
	let tone: Tone
	let scope: Scope
	let source: Source
	let summary: String
	let details: String?

	init(
		id: UUID = UUID(),
		tone: Tone,
		scope: Scope = .general,
		source: Source = .accountAction,
		summary: String,
		details: String? = nil
	) {
		self.id = id
		self.tone = tone
		self.scope = scope
		self.source = source
		self.summary = summary
		self.details = details
	}

	static func success(
		_ summary: String,
		source: Source = .accountAction
	) -> Self {
		Self(tone: .success, source: source, summary: summary)
	}

	static func information(
		_ summary: String,
		source: Source = .accountAction
	) -> Self {
		Self(tone: .information, source: source, summary: summary)
	}

	static func error(
		_ summary: String,
		details: String,
		scope: Scope = .general,
		source: Source = .accountAction
	) -> Self {
		Self(
			tone: .error,
			scope: scope,
			source: source,
			summary: summary,
			details: details
		)
	}

	func hasSamePresentation(as other: Self) -> Bool {
		tone == other.tone
			&& scope == other.scope
			&& source == other.source
			&& summary == other.summary
			&& details == other.details
	}

	var copyText: String {
		details ?? summary
	}

	var automaticDismissalDelay: Duration? {
		switch tone {
		case .success, .information:
			return .seconds(4)
		case .error:
			return nil
		}
	}
}

extension AccountNotice {
	static func resetCreditOutcome(
		_ outcome: ResetCreditConsumeOutcome,
		refreshError: String? = nil
	) -> Self {
		let result: Self
		switch outcome {
		case .reset:
			result = .success("Usage restored", source: .resetCredit)
		case .alreadyRedeemed:
			result = .information("Card already used", source: .resetCredit)
		case .nothingToReset:
			result = .information("Nothing to reset", source: .resetCredit)
		case .noCredit:
			result = .information("No reset card available", source: .resetCredit)
		}

		guard let refreshError else {
			return result
		}

		if outcome == .reset {
			return .error(
				"Usage restored; refresh failed",
				details: refreshError,
				source: .resetCredit
			)
		}

		return .error(
			"Couldn’t refresh account status",
			details: "\(result.summary)\n\n\(refreshError)",
			source: .resetCredit
		)
	}
}
