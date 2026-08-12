import { ago } from "./time.mjs";

import { lifecycleMetrics, lifecyclePhaseMetrics } from "./fixture-lifecycle.mjs";
import { activeRun } from "./fixture-lanes.mjs";

export function historyLane(accounts) {
	const run = activeRun({
		accounts,
		attempt: 1,
		issue: "XY-430",
		operation: "completed",
		status: "succeeded",
		title: "Completed dashboard lane",
		processAlive: false,
		activeLease: false,
		childActivity: null,
	});
	run.updated_at = ago(7_200);
	run.last_run_activity_at = ago(7_200);
	run.last_protocol_activity_at = ago(7_260);
	run.last_progress_at = ago(7_260);
	run.process_alive = false;
	run.thread_status = "completed";

	const developmentPhase = lifecyclePhaseMetrics({
		phase: "development",
		label: "Development",
		attemptCount: 2,
		protocolEventCount: 42,
		childEventCount: 78,
		wallSeconds: 2_940,
		toolCallCount: 31,
		inputTokens: 6_800_000,
		outputTokens: 21_000,
		buckets: [
			{
				name: "Model",
				wall_seconds: 2_320,
				event_count: 38,
				tool_call_count: 0,
				input_tokens: 6_800_000,
				output_tokens: 21_000,
				output_bytes: 0,
			},
			{
				name: "Shell",
				wall_seconds: 410,
				event_count: 28,
				tool_call_count: 24,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 58_000,
			},
			{
				name: "Tracker",
				wall_seconds: 0,
				event_count: 12,
				tool_call_count: 7,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 8_200,
			},
		],
	});
	const reviewPhase = lifecyclePhaseMetrics({
		phase: "review",
		label: "Review",
		attemptCount: 1,
		protocolEventCount: 16,
		childEventCount: 24,
		wallSeconds: 1_080,
		toolCallCount: 9,
		inputTokens: 2_100_000,
		outputTokens: 8_400,
		buckets: [
			{
				name: "Model",
				wall_seconds: 820,
				event_count: 14,
				tool_call_count: 0,
				input_tokens: 2_100_000,
				output_tokens: 8_400,
				output_bytes: 0,
			},
			{
				name: "GitHub",
				wall_seconds: 130,
				event_count: 5,
				tool_call_count: 4,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 11_000,
			},
			{
				name: "Shell",
				wall_seconds: 130,
				event_count: 5,
				tool_call_count: 5,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 16_000,
			},
		],
	});
	const closeoutPhase = lifecyclePhaseMetrics({
		phase: "closeout",
		label: "Closeout",
		attemptCount: 1,
		protocolEventCount: 8,
		childEventCount: 12,
		wallSeconds: 480,
		toolCallCount: 5,
		inputTokens: 520_000,
		outputTokens: 2_100,
		buckets: [
			{
				name: "Model",
				wall_seconds: 300,
				event_count: 6,
				tool_call_count: 0,
				input_tokens: 520_000,
				output_tokens: 2_100,
				output_bytes: 0,
			},
			{
				name: "Shell",
				wall_seconds: 180,
				event_count: 6,
				tool_call_count: 5,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 9_000,
			},
		],
	});
	const phases = [developmentPhase, reviewPhase, closeoutPhase];
	const lifecycle = lifecycleMetrics({
		attemptCount: 4,
		capturedAttemptCount: 4,
		protocolEventCount: phases.reduce((total, phase) => total + phase.protocol_event_count, 0),
		childEventCount: phases.reduce((total, phase) => total + phase.child_event_count, 0),
		wallSeconds: phases.reduce((total, phase) => total + phase.wall_seconds, 0),
		toolCallCount: phases.reduce((total, phase) => total + phase.tool_call_count, 0),
		inputTokens: phases.reduce((total, phase) => total + phase.input_tokens_cumulative, 0),
		outputTokens: phases.reduce((total, phase) => total + phase.output_tokens_cumulative, 0),
		buckets: [
			{
				name: "Model",
				wall_seconds: 3_440,
				event_count: 58,
				tool_call_count: 0,
				input_tokens: 9_420_000,
				output_tokens: 31_500,
				output_bytes: 0,
			},
			{
				name: "Shell",
				wall_seconds: 720,
				event_count: 39,
				tool_call_count: 34,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 83_000,
			},
			{
				name: "GitHub",
				wall_seconds: 130,
				event_count: 5,
				tool_call_count: 4,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 11_000,
			},
			{
				name: "Tracker",
				wall_seconds: 0,
				event_count: 12,
				tool_call_count: 7,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 8_200,
			},
		],
	});
	lifecycle.phases = phases;

	return {
		project_id: "decodex-preview",
		issue_id: "issue-xy-430",
		issue_identifier: "XY-430",
		title: "Completed dashboard lane",
		issue_key: "XY-430",
		attempt_count: 4,
		lifecycle_metrics: lifecycle,
		ledger_outcome: {
			ledger_status: "present",
			final_outcome: "succeeded",
			final_event_type: "issue_closeout_complete",
			final_event_at: ago(7_200),
			summary: "Merged, closed out, and cleaned up.",
			pr_url: "https://github.com/acg-box/decodex/pull/430",
			commit_sha: "abc123def456",
			branch: "xy/xy-430-dashboard",
			closeout_status: "completed",
			needs_attention_reason: null,
			lifecycle_started_at: ago(12_000),
			lifecycle_finished_at: ago(7_200),
			lifecycle_elapsed_seconds: 4_800,
			record_count: 8,
		},
		latest_run: run,
		attempts: [
			{ ...run, run_id: "xy-430-attempt-1-mock", attempt_number: 1, status: "failed" },
			{ ...run, run_id: "xy-430-attempt-2-mock", attempt_number: 2, status: "succeeded" },
			{ ...run, run_id: "xy-430-review-1-mock", attempt_number: 3, status: "succeeded" },
			{ ...run, run_id: "xy-430-closeout-1-mock", attempt_number: 4, status: "succeeded" },
		],
	};
}
