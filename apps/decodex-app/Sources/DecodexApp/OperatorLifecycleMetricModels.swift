import Foundation

struct OperatorLifecycleMetricBucket: Decodable, Sendable {
	let name: String
	let wallSeconds: Int
	let eventCount: Int
	let toolCallCount: Int
	let inputTokens: Int
	let outputTokens: Int
	let outputBytes: Int

	enum CodingKeys: String, CodingKey {
		case name
		case wallSeconds = "wall_seconds"
		case eventCount = "event_count"
		case toolCallCount = "tool_call_count"
		case inputTokens = "input_tokens"
		case outputTokens = "output_tokens"
		case outputBytes = "output_bytes"
	}
}

struct OperatorLifecycleMetricPhase: Decodable, Sendable {
	let phase: String?
	let label: String?
	let attemptCount: Int
	let runCount: Int
	let capturedAttemptCount: Int
	let missingAttemptCount: Int
	let protocolEventCount: Int
	let childEventCount: Int
	let wallSeconds: Int
	let toolCallCount: Int
	let inputTokensCurrent: Int?
	let inputTokensPeak: Int?
	let inputTokensCumulative: Int
	let outputTokensCumulative: Int
	let largestToolOutputBytes: Int?
	let largestToolOutputTool: String?
	let buckets: [OperatorLifecycleMetricBucket]

	enum CodingKeys: String, CodingKey {
		case phase
		case label
		case attemptCount = "attempt_count"
		case runCount = "run_count"
		case capturedAttemptCount = "captured_attempt_count"
		case missingAttemptCount = "missing_attempt_count"
		case protocolEventCount = "protocol_event_count"
		case childEventCount = "child_event_count"
		case wallSeconds = "wall_seconds"
		case toolCallCount = "tool_call_count"
		case inputTokensCurrent = "input_tokens_current"
		case inputTokensPeak = "input_tokens_peak"
		case inputTokensCumulative = "input_tokens_cumulative"
		case outputTokensCumulative = "output_tokens_cumulative"
		case largestToolOutputBytes = "largest_tool_output_bytes"
		case largestToolOutputTool = "largest_tool_output_tool"
		case buckets
	}
}

struct OperatorLifecycleMetrics: Decodable, Sendable {
	let attemptCount: Int
	let runCount: Int
	let capturedAttemptCount: Int
	let missingAttemptCount: Int
	let protocolEventCount: Int
	let childEventCount: Int
	let wallSeconds: Int
	let toolCallCount: Int
	let inputTokensCurrent: Int?
	let inputTokensPeak: Int?
	let inputTokensCumulative: Int
	let outputTokensCumulative: Int
	let largestToolOutputBytes: Int?
	let largestToolOutputTool: String?
	let buckets: [OperatorLifecycleMetricBucket]
	let phases: [OperatorLifecycleMetricPhase]

	enum CodingKeys: String, CodingKey {
		case attemptCount = "attempt_count"
		case runCount = "run_count"
		case capturedAttemptCount = "captured_attempt_count"
		case missingAttemptCount = "missing_attempt_count"
		case protocolEventCount = "protocol_event_count"
		case childEventCount = "child_event_count"
		case wallSeconds = "wall_seconds"
		case toolCallCount = "tool_call_count"
		case inputTokensCurrent = "input_tokens_current"
		case inputTokensPeak = "input_tokens_peak"
		case inputTokensCumulative = "input_tokens_cumulative"
		case outputTokensCumulative = "output_tokens_cumulative"
		case largestToolOutputBytes = "largest_tool_output_bytes"
		case largestToolOutputTool = "largest_tool_output_tool"
		case buckets
		case phases
	}
}
