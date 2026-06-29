import Foundation

struct OperatorChildAgentActivity: Decodable, Sendable {
	let currentBucket: String?
	let currentDetail: String?
	let currentElapsedSeconds: Int?
	let currentStartedUnixEpoch: Int64?
	let eventCount: Int
	let inputTokensCumulative: Int
	let inputTokensCurrent: Int?
	let inputTokensMax: Int?
	let largestToolOutputBytes: Int?
	let largestToolOutputTool: String?
	let outputTokensCumulative: Int
	let toolCallCount: Int
	let wallSeconds: Int
	let buckets: [OperatorChildAgentBucket]

	enum CodingKeys: String, CodingKey {
		case currentBucket = "current_bucket"
		case currentDetail = "current_detail"
		case currentElapsedSeconds = "current_elapsed_seconds"
		case currentStartedUnixEpoch = "current_started_unix_epoch"
		case eventCount = "event_count"
		case inputTokensCumulative = "input_tokens_cumulative"
		case inputTokensCurrent = "input_tokens_current"
		case inputTokensMax = "input_tokens_max"
		case largestToolOutputBytes = "largest_tool_output_bytes"
		case largestToolOutputTool = "largest_tool_output_tool"
		case outputTokensCumulative = "output_tokens_cumulative"
		case toolCallCount = "tool_call_count"
		case wallSeconds = "wall_seconds"
		case buckets
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)

		currentBucket = try container.decodeIfPresent(String.self, forKey: .currentBucket)
		currentDetail = try container.decodeIfPresent(String.self, forKey: .currentDetail)
		currentElapsedSeconds = try container.decodeIfPresent(Int.self, forKey: .currentElapsedSeconds)
		currentStartedUnixEpoch = try container.decodeIfPresent(Int64.self, forKey: .currentStartedUnixEpoch)
		eventCount = try container.decodeIfPresent(Int.self, forKey: .eventCount) ?? 0
		inputTokensCumulative = try container.decodeIfPresent(Int.self, forKey: .inputTokensCumulative) ?? 0
		inputTokensCurrent = try container.decodeIfPresent(Int.self, forKey: .inputTokensCurrent)
		inputTokensMax = try container.decodeIfPresent(Int.self, forKey: .inputTokensMax)
		largestToolOutputBytes = try container.decodeIfPresent(Int.self, forKey: .largestToolOutputBytes)
		largestToolOutputTool = try container.decodeIfPresent(String.self, forKey: .largestToolOutputTool)
		outputTokensCumulative = try container.decodeIfPresent(Int.self, forKey: .outputTokensCumulative) ?? 0
		toolCallCount = try container.decodeIfPresent(Int.self, forKey: .toolCallCount) ?? 0
		wallSeconds = try container.decodeIfPresent(Int.self, forKey: .wallSeconds) ?? 0
		buckets = try container.decodeIfPresent([OperatorChildAgentBucket].self, forKey: .buckets) ?? []
	}

	func currentElapsedSeconds(at now: Date) -> Int? {
		var candidates = [Int]()
		if let currentElapsedSeconds {
			candidates.append(currentElapsedSeconds)
		}
		if let currentStartedUnixEpoch {
			let liveElapsed = Int(now.timeIntervalSince1970.rounded(.down)) - Int(currentStartedUnixEpoch)

			candidates.append(max(0, liveElapsed))
		}

		return candidates.max()
	}

	func wallSeconds(at now: Date) -> Int {
		wallSeconds + currentElapsedDelta(at: now)
	}

	func wallSeconds(
		for bucket: OperatorChildAgentBucket,
		at now: Date
	) -> Int {
		guard let currentBucket, bucket.name.caseInsensitiveCompare(currentBucket) == .orderedSame else {
			return bucket.wallSeconds
		}

		return bucket.wallSeconds + currentElapsedDelta(at: now)
	}

	private func currentElapsedDelta(at now: Date) -> Int {
		guard let baselineElapsed = currentElapsedSeconds, let liveElapsed = currentElapsedSeconds(at: now) else {
			return 0
		}

		return max(0, liveElapsed - baselineElapsed)
	}
}

struct OperatorChildAgentBucket: Decodable, Identifiable, Sendable {
	let name: String
	let eventCount: Int
	let inputTokens: Int
	let outputBytes: Int
	let outputTokens: Int
	let toolCallCount: Int
	let wallSeconds: Int

	var id: String {
		name
	}

	enum CodingKeys: String, CodingKey {
		case name
		case eventCount = "event_count"
		case inputTokens = "input_tokens"
		case outputBytes = "output_bytes"
		case outputTokens = "output_tokens"
		case toolCallCount = "tool_call_count"
		case wallSeconds = "wall_seconds"
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)

		name = try container.decodeIfPresent(String.self, forKey: .name) ?? "Activity"
		eventCount = try container.decodeIfPresent(Int.self, forKey: .eventCount) ?? 0
		inputTokens = try container.decodeIfPresent(Int.self, forKey: .inputTokens) ?? 0
		outputBytes = try container.decodeIfPresent(Int.self, forKey: .outputBytes) ?? 0
		outputTokens = try container.decodeIfPresent(Int.self, forKey: .outputTokens) ?? 0
		toolCallCount = try container.decodeIfPresent(Int.self, forKey: .toolCallCount) ?? 0
		wallSeconds = try container.decodeIfPresent(Int.self, forKey: .wallSeconds) ?? 0
	}
}
