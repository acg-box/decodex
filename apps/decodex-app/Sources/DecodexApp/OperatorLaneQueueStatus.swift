import Foundation

struct OperatorQueuedIssueStatus: Decodable, Sendable {
	let classification: String?

	var isClosed: Bool {
		classification == "closed"
	}

	enum CodingKeys: String, CodingKey {
		case classification
	}
}

struct OperatorPostReviewLaneStatus: Decodable, Sendable {
	let classification: String?
	let shadowedByCurrentLane: Bool

	var isReadyToLand: Bool {
		classification == "ready_to_land" && shadowedByCurrentLane == false
	}

	enum CodingKeys: String, CodingKey {
		case classification
		case shadowedByCurrentLane = "shadowed_by_current_lane"
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		classification = try container.decodeIfPresent(String.self, forKey: .classification)
		shadowedByCurrentLane = try container.decode(Bool.self, forKey: .shadowedByCurrentLane)
	}
}
