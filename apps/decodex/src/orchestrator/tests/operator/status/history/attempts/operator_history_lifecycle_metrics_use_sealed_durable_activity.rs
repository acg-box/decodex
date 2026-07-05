use crate::orchestrator::tests::operator::status::{
	self, StateStore, history::attempts, orchestrator,
};

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
			Some(&attempts::unsealed_history_lane_child_activity()),
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
