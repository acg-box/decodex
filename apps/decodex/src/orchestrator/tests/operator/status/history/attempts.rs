mod operator_history_lifecycle_metrics_use_sealed_durable_activity;
mod operator_status_history_lanes_group_attempts_by_issue;
mod operator_status_history_limit_applies_after_current_lanes_are_split_out;

use crate::orchestrator::tests::operator::status::{
	ChildAgentActivitySummary, ReviewPolicyCheckpointInput, StateStore, TEST_SERVICE_ID, state,
};

fn history_lane_child_activity(
	wall_seconds: i64,
	event_count: i64,
	tool_call_count: i64,
	input_tokens: i64,
	output_tokens: i64,
) -> ChildAgentActivitySummary {
	ChildAgentActivitySummary {
		buckets: vec![state::ChildAgentActivityBucket {
			name: String::from("Model"),
			wall_seconds,
			event_count,
			tool_call_count,
			input_tokens,
			output_tokens,
			output_bytes: 0,
		}],
		wall_seconds,
		event_count,
		tool_call_count,
		input_tokens_cumulative: input_tokens,
		output_tokens_cumulative: output_tokens,
		..ChildAgentActivitySummary::default()
	}
}

fn unsealed_history_lane_child_activity() -> ChildAgentActivitySummary {
	let mut activity = history_lane_child_activity(12, 3, 1, 100, 30);

	activity.current_bucket = Some(String::from("Model"));
	activity.current_detail = Some(String::from("model output"));
	activity.current_started_unix_epoch = Some(1);
	activity.current_elapsed_seconds = Some(11);

	activity
}

fn seed_grouped_history_lane_lifecycle_metrics(state_store: &StateStore, issue_id: &str) {
	let first_activity = history_lane_child_activity(10, 2, 1, 100, 30);
	let second_activity = history_lane_child_activity(20, 3, 4, 200, 40);

	state_store
		.record_run_activity_summary("xy-323-attempt-1-1777361523", 1, Some(&first_activity), None)
		.expect("first activity summary should record");
	state_store
		.record_run_activity_summary("xy-323-attempt-2-1777361550", 2, Some(&second_activity), None)
		.expect("second activity summary should record");
	state_store
		.append_event("xy-323-attempt-2-1777361550", 1, "turn/started", "{}")
		.expect("second protocol event should record");
	state_store
		.append_event("xy-323-attempt-2-1777361550", 2, "turn/completed", "{}")
		.expect("third protocol event should record");
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: TEST_SERVICE_ID,
			issue_id,
			run_id: "xy-323-attempt-2-1777361550",
			attempt_number: 2,
			phase: "handoff",
			review_level: "standard",
			status: "clean",
			head_sha: "2222222222222222222222222222222222222222",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("second attempt review checkpoint should record");
}
