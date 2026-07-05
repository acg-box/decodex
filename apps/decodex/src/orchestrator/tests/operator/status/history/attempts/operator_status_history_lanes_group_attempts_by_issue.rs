use crate::orchestrator::tests::operator::status::{
	self, StateStore, history::attempts, orchestrator,
};

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

	attempts::seed_grouped_history_lane_lifecycle_metrics(&state_store, &first_issue.id);

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
