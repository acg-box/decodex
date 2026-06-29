import Foundation

struct OperatorRunAccountSummary: Decodable, Sendable {
	let accountFingerprint: String
	let email: String?
	let status: String?

	var isSelected: Bool {
		status?.caseInsensitiveCompare("selected") == .orderedSame
	}

	func matches(_ account: CodexAccount) -> Bool {
		if accountFingerprint.isEmpty == false, accountFingerprint == account.accountFingerprint {
			return true
		}
		if let email, let accountEmail = account.email {
			return email.caseInsensitiveCompare(accountEmail) == .orderedSame
		}

		return false
	}

	enum CodingKeys: String, CodingKey {
		case accountFingerprint = "account_fingerprint"
		case email
		case accountEmail = "account_email"
		case status
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)

		accountFingerprint = try container.decodeIfPresent(String.self, forKey: .accountFingerprint) ?? ""
		email = try container.decodeIfPresent(String.self, forKey: .email)
			?? container.decodeIfPresent(String.self, forKey: .accountEmail)
		status = try container.decodeIfPresent(String.self, forKey: .status)
	}
}

struct OperatorSnapshotPresentation: Decodable, Sendable {
	let schema: String?
	let currentLaneCards: [OperatorCurrentLaneCard]

	enum CodingKeys: String, CodingKey {
		case schema
		case currentLaneCards = "current_lane_cards"
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)

		schema = try container.decodeIfPresent(String.self, forKey: .schema)
		currentLaneCards = try container.decodeIfPresent(
			[OperatorCurrentLaneCard].self,
			forKey: .currentLaneCards
		) ?? []
	}
}

struct OperatorCurrentLaneCard: Decodable, Identifiable, Sendable {
	let id: String
	let runID: String
	let issueID: String?
	let issueIdentifier: String?
	let projectID: String?
	let title: String
	let detail: String
	let tone: String
	let countsAsRunning: Bool
	let needsAttention: Bool
	let isWaiting: Bool
	let assignedAccountFingerprints: [String]
	let assignedAccountEmails: [String]
	let run: OperatorRunStatus

	func isAssigned(to account: CodexAccount) -> Bool {
		if assignedAccountFingerprints.contains(where: { $0 == account.accountFingerprint }) {
			return true
		}
		guard let accountEmail = account.email else {
			return false
		}

		return assignedAccountEmails.contains {
			$0.caseInsensitiveCompare(accountEmail) == .orderedSame
		}
	}

	enum CodingKeys: String, CodingKey {
		case id
		case runID = "run_id"
		case issueID = "issue_id"
		case issueIdentifier = "issue_identifier"
		case projectID = "project_id"
		case title
		case detail
		case tone
		case countsAsRunning = "counts_as_running"
		case needsAttention = "needs_attention"
		case isWaiting = "is_waiting"
		case assignedAccountFingerprints = "assigned_account_fingerprints"
		case assignedAccountEmails = "assigned_account_emails"
		case run
	}
}
