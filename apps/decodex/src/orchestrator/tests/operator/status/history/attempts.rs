use crate::orchestrator::tests::operator::status::{
	self, ChildAgentActivitySummary, ReviewPolicyCheckpointInput, StateStore, TEST_SERVICE_ID,
	orchestrator, state,
};

#[test]
fn operator_status_history_limit_applies_after_current_lanes_are_split_out() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let active_issue = status::sample_issue_with_sort_fields(
		"issue-1",
		"PUB-101",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let failed_issue = status::sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T04:17:17.133Z",
	);

	state_store
		.record_run_attempt("run-active", &active_issue.id, 1, "running")
		.expect("current lane should record");
	state_store
		.upsert_lease("pubfi", &active_issue.id, "run-active", "In Progress")
		.expect("run lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&active_issue.id,
			"x/pubfi-pub-101",
			&config.worktree_root().join(&active_issue.identifier).display().to_string(),
		)
		.expect("active worktree should record");
	state_store
		.record_run_attempt("run-failed", &failed_issue.id, 1, "failed")
		.expect("failed run should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&failed_issue.id,
			"x/pubfi-pub-102",
			&config.worktree_root().join(&failed_issue.identifier).display().to_string(),
		)
		.expect("failed worktree should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 1)
		.expect("snapshot should build");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(snapshot.run_limit, 1);
	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(snapshot.recent_runs.len(), 2);
	assert_eq!(snapshot.history_lanes.len(), 1);
	assert_eq!(snapshot.history_lanes[0].attempt_count, 1);
	assert!(rendered.contains(
		"Run ledger shown: 1 issue lanes from 1 history attempts (current lanes inline)"
	));
	assert_eq!(rendered.matches("run_id: run-active").count(), 1);
	assert_eq!(rendered.matches("run_id: run-failed").count(), 1);

	let history_index = rendered.find("Run Ledger").expect("history section should render");
	let failed_index = rendered.find("run_id: run-failed").expect("failed run should render");

	assert!(
		failed_index > history_index,
		"history-only run should remain visible after current lane overlap is hidden"
	);
}

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

#[test]
fn operator_status_history_lanes_group_attempts_by_issue() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let first_issue = status::sample_issue_with_sort_fields(
		"issue-1",
		"XY-323",
		"Done",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let second_issue = status::sample_issue_with_sort_fields(
		"issue-2",
		"XY-330",
		"Done",
		&[],
		Some(3),
		"2026-03-13T04:17:17.133Z",
	);

	state_store
		.record_run_attempt("xy-323-attempt-1-1777361523", &first_issue.id, 1, "failed")
		.expect("first attempt should record");
	state_store
		.record_run_attempt("xy-323-attempt-2-1777361550", &first_issue.id, 2, "succeeded")
		.expect("second attempt should record");
	state_store
		.record_run_attempt("xy-330-attempt-1-1777361600", &second_issue.id, 1, "succeeded")
		.expect("other issue attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&first_issue.id,
			"x/decodex-xy-323",
			&config.worktree_root().join(&first_issue.identifier).display().to_string(),
		)
		.expect("first issue worktree should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&second_issue.id,
			"x/decodex-xy-330",
			&config.worktree_root().join(&second_issue.identifier).display().to_string(),
		)
		.expect("second issue worktree should record");

	seed_grouped_history_lane_lifecycle_metrics(&state_store, &first_issue.id);

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let grouped_lane = snapshot
		.history_lanes
		.iter()
		.find(|lane| lane.issue_key == "XY-323")
		.expect("first issue should have a grouped history lane");

	assert_eq!(snapshot.recent_runs.len(), 3);
	assert_eq!(snapshot.history_lanes.len(), 2);
	assert_eq!(grouped_lane.attempt_count, 2);
	assert_eq!(grouped_lane.latest_run.run_id, "xy-323-attempt-2-1777361550");
	assert_eq!(grouped_lane.lifecycle_metrics.attempt_count, 2);
	assert_eq!(grouped_lane.lifecycle_metrics.captured_attempt_count, 2);
	assert_eq!(grouped_lane.lifecycle_metrics.missing_attempt_count, 0);
	assert_eq!(grouped_lane.lifecycle_metrics.protocol_event_count, 2);
	assert_eq!(grouped_lane.lifecycle_metrics.child_event_count, 5);
	assert_eq!(grouped_lane.lifecycle_metrics.wall_seconds, 30);
	assert_eq!(grouped_lane.lifecycle_metrics.tool_call_count, 5);
	assert_eq!(grouped_lane.lifecycle_metrics.input_tokens_cumulative, 300);
	assert_eq!(grouped_lane.lifecycle_metrics.output_tokens_cumulative, 70);
	assert_eq!(grouped_lane.lifecycle_metrics.buckets[0].name, "Model");
	assert_eq!(grouped_lane.lifecycle_metrics.buckets[0].wall_seconds, 30);
	assert_eq!(grouped_lane.lifecycle_metrics.phases.len(), 2);
	assert_eq!(grouped_lane.lifecycle_metrics.phases[0].phase, "development");
	assert_eq!(grouped_lane.lifecycle_metrics.phases[0].label, "Development");
	assert_eq!(grouped_lane.lifecycle_metrics.phases[0].attempt_count, 1);
	assert_eq!(grouped_lane.lifecycle_metrics.phases[0].captured_attempt_count, 1);
	assert_eq!(grouped_lane.lifecycle_metrics.phases[0].protocol_event_count, 0);
	assert_eq!(grouped_lane.lifecycle_metrics.phases[0].child_event_count, 2);
	assert_eq!(grouped_lane.lifecycle_metrics.phases[0].wall_seconds, 10);
	assert_eq!(grouped_lane.lifecycle_metrics.phases[0].tool_call_count, 1);
	assert_eq!(grouped_lane.lifecycle_metrics.phases[0].input_tokens_cumulative, 100);
	assert_eq!(grouped_lane.lifecycle_metrics.phases[0].output_tokens_cumulative, 30);
	assert_eq!(grouped_lane.lifecycle_metrics.phases[1].phase, "review");
	assert_eq!(grouped_lane.lifecycle_metrics.phases[1].label, "Review");
	assert_eq!(grouped_lane.lifecycle_metrics.phases[1].attempt_count, 1);
	assert_eq!(grouped_lane.lifecycle_metrics.phases[1].captured_attempt_count, 1);
	assert_eq!(grouped_lane.lifecycle_metrics.phases[1].protocol_event_count, 2);
	assert_eq!(grouped_lane.lifecycle_metrics.phases[1].child_event_count, 3);
	assert_eq!(grouped_lane.lifecycle_metrics.phases[1].wall_seconds, 20);
	assert_eq!(grouped_lane.lifecycle_metrics.phases[1].tool_call_count, 4);
	assert_eq!(grouped_lane.lifecycle_metrics.phases[1].input_tokens_cumulative, 200);
	assert_eq!(grouped_lane.lifecycle_metrics.phases[1].output_tokens_cumulative, 40);
	assert!(rendered.contains("Run ledger shown: 2 issue lanes from 3 history attempts"));
	assert!(rendered.contains("issue: XY-323"));
	assert!(rendered.contains("attempts: 2"));
	assert!(rendered.contains(
		"lifecycle_metrics: attempts=2; sources=recorded:2,recovered:0,current_snapshot:0; captured=2/2; missing=0; protocol_events=2"
	));
	assert!(rendered.contains("lifecycle_bucket_breakdown"));
	assert!(rendered.contains(
		"lifecycle_bucket: Development lifecycle_bucket_key: development attempts: 1 sources: recorded=1 recovered=0 current_snapshot=0"
	));
	assert!(rendered.contains(
		"lifecycle_bucket: Review lifecycle_bucket_key: review attempts: 1 sources: recorded=1 recovered=0 current_snapshot=0"
	));
}

#[test]
fn operator_history_lifecycle_metrics_use_sealed_durable_activity() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-324",
		"XY-324",
		"Done",
		&[],
		Some(3),
		"2026-03-13T04:18:17.133Z",
	);

	state_store
		.record_run_attempt("xy-324-attempt-1-1777361523", &issue.id, 1, "failed")
		.expect("failed run should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/decodex-xy-324",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.record_run_activity_summary(
			"xy-324-attempt-1-1777361523",
			1,
			Some(&unsealed_history_lane_child_activity()),
			None,
		)
		.expect("activity summary should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let grouped_lane = snapshot
		.history_lanes
		.iter()
		.find(|lane| lane.issue_key == "XY-324")
		.expect("history lane should exist");
	let activity = grouped_lane
		.latest_run
		.child_agent_activity
		.as_ref()
		.expect("sealed activity should remain available");
	let model_bucket = activity
		.buckets
		.iter()
		.find(|bucket| bucket.name == "Model")
		.expect("model bucket should remain available");

	assert_eq!(activity.current_bucket, None);
	assert_eq!(activity.current_started_unix_epoch, None);
	assert_eq!(activity.current_elapsed_seconds, None);
	assert_eq!(model_bucket.wall_seconds, 12);
	assert_eq!(grouped_lane.lifecycle_metrics.wall_seconds, 12);
	assert_eq!(grouped_lane.lifecycle_metrics.buckets[0].wall_seconds, 12);
}
