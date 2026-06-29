import Foundation

struct OperatorDashboardSocketEvent: Decodable, Sendable {
	let type: String
	let payload: OperatorDashboardSocketPayload?
}

struct OperatorDashboardSocketPayload: Decodable, Sendable {
	let emittedAtUnixEpoch: Int64?
	let snapshotPublishedAtUnixEpoch: Int64?
	let snapshot: OperatorSnapshotResponse?
	let presentation: OperatorSnapshotPresentation?

	var emittedAt: Date? {
		date(fromUnixEpoch: emittedAtUnixEpoch)
	}

	var snapshotPublishedAt: Date? {
		date(fromUnixEpoch: snapshotPublishedAtUnixEpoch)
	}

	enum CodingKeys: String, CodingKey {
		case emittedAtUnixEpoch
		case snapshotPublishedAtUnixEpoch
		case snapshot
		case presentation
	}
}

private func date(fromUnixEpoch value: Int64?) -> Date? {
	guard let value else {
		return nil
	}

	return Date(timeIntervalSince1970: TimeInterval(value))
}
