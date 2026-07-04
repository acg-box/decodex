export function childAgentActivity() {
	return {
		buckets: [
			{
				name: "Model",
				wall_seconds: 693,
				event_count: 12,
				tool_call_count: 0,
				input_tokens: 4_270_000,
				output_tokens: 12_000,
				output_bytes: 0,
			},
			{
				name: "Shell",
				wall_seconds: 96,
				event_count: 10,
				tool_call_count: 6,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 24_000,
			},
			{
				name: "Browser/Image",
				wall_seconds: 41,
				event_count: 6,
				tool_call_count: 3,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 180_000,
			},
			{
				name: "Tracker",
				wall_seconds: 0,
				event_count: 2,
				tool_call_count: 2,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 2_100,
			},
		],
		current_bucket: "Model",
		current_detail: "waiting after tool output",
		current_started_unix_epoch: null,
		current_elapsed_seconds: 652,
		wall_seconds: 830,
		event_count: 30,
		tool_call_count: 11,
		input_tokens_current: 105_000,
		input_tokens_max: 128_000,
		input_tokens_cumulative: 4_270_000,
		output_tokens_cumulative: 12_000,
		largest_tool_output_bytes: 180_000,
		largest_tool_output_tool: "view_image",
		large_output_warnings: ["view_image repeated 3 large outputs; largest 180000 bytes"],
	};
}
export function lifecycleMetrics({
	attemptCount,
	capturedAttemptCount = attemptCount,
	protocolEventCount,
	childEventCount,
	wallSeconds,
	toolCallCount,
	inputTokens,
	outputTokens,
	buckets = [],
}) {
	return {
		attempt_count: attemptCount,
		run_count: attemptCount,
		captured_attempt_count: capturedAttemptCount,
		missing_attempt_count: Math.max(0, attemptCount - capturedAttemptCount),
		protocol_event_count: protocolEventCount,
		child_event_count: childEventCount,
		wall_seconds: wallSeconds,
		tool_call_count: toolCallCount,
		input_tokens_cumulative: inputTokens,
		output_tokens_cumulative: outputTokens,
		largest_tool_output_bytes: 180_000,
		largest_tool_output_tool: "view_image",
		buckets,
	};
}
export function lifecyclePhaseMetrics({ phase, label, ...metrics }) {
	return {
		phase,
		label,
		...lifecycleMetrics(metrics),
	};
}
export function activeRunLifecycleMetrics(childActivity, { attemptCount = 1, phase = "development", label = "Development" } = {}) {
	if (!childActivity) {
		return lifecycleMetrics({
			attemptCount: 0,
			capturedAttemptCount: 0,
			protocolEventCount: 0,
			childEventCount: 0,
			wallSeconds: 0,
			toolCallCount: 0,
			inputTokens: 0,
			outputTokens: 0,
			buckets: [],
		});
	}
	const phaseMetrics = lifecyclePhaseMetrics({
		phase,
		label,
		attemptCount,
		protocolEventCount: childActivity.event_count,
		childEventCount: childActivity.event_count,
		wallSeconds: childActivity.wall_seconds,
		toolCallCount: childActivity.tool_call_count,
		inputTokens: childActivity.input_tokens_cumulative,
		outputTokens: childActivity.output_tokens_cumulative,
		buckets: childActivity.buckets,
	});
	const total = {
		...phaseMetrics,
		phase: undefined,
		label: undefined,
		phases: [phaseMetrics],
	};

	total.input_tokens_current = childActivity.input_tokens_current;
	total.input_tokens_peak = childActivity.input_tokens_max;
	total.large_output_warnings = childActivity.large_output_warnings || [];
	total.largest_tool_output_bytes = childActivity.largest_tool_output_bytes;
	total.largest_tool_output_tool = childActivity.largest_tool_output_tool;
	phaseMetrics.input_tokens_current = childActivity.input_tokens_current;
	phaseMetrics.input_tokens_peak = childActivity.input_tokens_max;
	phaseMetrics.large_output_warnings = childActivity.large_output_warnings || [];
	phaseMetrics.largest_tool_output_bytes = childActivity.largest_tool_output_bytes;
	phaseMetrics.largest_tool_output_tool = childActivity.largest_tool_output_tool;

	delete total.phase;
	delete total.label;

	return total;
}
export function activeReviewLifecycleMetrics(currentActivity) {
	const developmentPhase = lifecyclePhaseMetrics({
		phase: "development",
		label: "Development",
		attemptCount: 1,
		protocolEventCount: 18,
		childEventCount: 24,
		wallSeconds: 910,
		toolCallCount: 7,
		inputTokens: 2_850_000,
		outputTokens: 8_500,
		buckets: [
			{
				name: "Model",
				wall_seconds: 620,
				event_count: 11,
				tool_call_count: 0,
				input_tokens: 2_850_000,
				output_tokens: 8_500,
				output_bytes: 0,
			},
			{
				name: "Shell",
				wall_seconds: 220,
				event_count: 9,
				tool_call_count: 5,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 34_000,
			},
			{
				name: "Tracker",
				wall_seconds: 0,
				event_count: 4,
				tool_call_count: 2,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 3_200,
			},
		],
	});
	developmentPhase.largest_tool_output_bytes = 34_000;
	developmentPhase.largest_tool_output_tool = "shell";
	developmentPhase.large_output_warnings = [];
	const reviewPhase = activeRunLifecycleMetrics(currentActivity, {
		attemptCount: 1,
		phase: "review",
		label: "Review",
	}).phases[0];
	const phases = [developmentPhase, reviewPhase];
	const bucketTotals = new Map();

	for (const phase of phases) {
		for (const bucket of phase.buckets || []) {
			const total =
				bucketTotals.get(bucket.name) ||
				{
					name: bucket.name,
					wall_seconds: 0,
					event_count: 0,
					tool_call_count: 0,
					input_tokens: 0,
					output_tokens: 0,
					output_bytes: 0,
				};
			total.wall_seconds += bucket.wall_seconds || 0;
			total.event_count += bucket.event_count || 0;
			total.tool_call_count += bucket.tool_call_count || 0;
			total.input_tokens += bucket.input_tokens || 0;
			total.output_tokens += bucket.output_tokens || 0;
			total.output_bytes += bucket.output_bytes || 0;
			bucketTotals.set(bucket.name, total);
		}
	}

	const total = lifecycleMetrics({
		attemptCount: 2,
		protocolEventCount: phases.reduce((count, phase) => count + phase.protocol_event_count, 0),
		childEventCount: phases.reduce((count, phase) => count + phase.child_event_count, 0),
		wallSeconds: phases.reduce((count, phase) => count + phase.wall_seconds, 0),
		toolCallCount: phases.reduce((count, phase) => count + phase.tool_call_count, 0),
		inputTokens: phases.reduce((count, phase) => count + phase.input_tokens_cumulative, 0),
		outputTokens: phases.reduce((count, phase) => count + phase.output_tokens_cumulative, 0),
		buckets: Array.from(bucketTotals.values()).sort((left, right) => right.wall_seconds - left.wall_seconds),
	});

	total.input_tokens_current = currentActivity.input_tokens_current;
	total.input_tokens_peak = Math.max(currentActivity.input_tokens_max, 128_000);
	total.large_output_warnings = currentActivity.large_output_warnings || [];
	total.largest_tool_output_bytes = currentActivity.largest_tool_output_bytes;
	total.largest_tool_output_tool = currentActivity.largest_tool_output_tool;
	total.phases = phases;

	return total;
}
