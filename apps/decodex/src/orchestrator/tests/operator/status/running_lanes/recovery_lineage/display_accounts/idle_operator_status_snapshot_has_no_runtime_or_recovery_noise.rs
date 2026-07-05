use crate::orchestrator::tests::operator::status::running_lanes::{
	self, FakeTracker, StateStore, orchestrator,
};

#[test]
fn idle_operator_status_snapshot_has_no_runtime_or_recovery_noise() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("idle snapshot should build");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert_eq!(snapshot.project_id, "pubfi");
	assert_eq!(snapshot.run_limit, 10);
	assert!(snapshot.warnings.is_empty(), "idle snapshot warnings: {:?}", snapshot.warnings);
	assert!(snapshot.current_lanes.is_empty(), "idle snapshot should have no current lanes");
	assert!(snapshot.recent_runs.is_empty(), "idle snapshot should have no run history");
	assert!(snapshot.history_lanes.is_empty(), "idle snapshot should have no run ledger lanes");
	assert!(
		snapshot.queued_candidates.is_empty(),
		"idle snapshot should have no queued candidates"
	);
	assert!(snapshot.worktrees.is_empty(), "idle snapshot should have no recovery worktrees");
	assert!(
		snapshot.post_review_lanes.is_empty(),
		"idle snapshot should have no retained post-review lanes"
	);

	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(project.retained_worktree_count, 0);
	assert_eq!(project.waiting_lane_count, 0);
	assert_eq!(project.attention_count, 0);
	assert_eq!(project.cleanup_blocked_count, 0);
	assert_eq!(project.cleanup_pending_count, 0);
	assert_eq!(project.connector_state, "ok");
	assert_eq!(project.last_activity_at, None);

	for field in [
		"warnings",
		"warning_details",
		"current_lanes",
		"recent_runs",
		"history_lanes",
		"queued_candidates",
		"worktrees",
		"post_review_lanes",
	] {
		assert_eq!(
			snapshot_json[field],
			serde_json::json!([]),
			"idle operator snapshot field {field} should serialize as an empty array",
		);
	}

	assert!(rendered.contains("Warnings: 0"));
	assert!(rendered.contains("Running lanes: 0"));
	assert!(rendered.contains("Run ledger shown: 0 issue lanes from 0 history attempts"));
	assert!(rendered.contains("Backlog: 0"));
	assert!(rendered.contains("Claimed queue echoes: 0"));
	assert!(rendered.contains("Stale closed queue labels: 0"));
	assert!(rendered.contains("Recovery worktrees: 0"));
	assert!(rendered.contains("Post-review lanes: 0"));
	assert!(rendered.contains("\nCurrent Lanes\n- none\n"));
	assert!(rendered.contains("\nRun Ledger\n- none\n"));
	assert!(rendered.contains("\nBacklog\n- none\n"));
	assert!(rendered.contains("\nClaimed Queue Echoes\n- none\n"));
	assert!(rendered.contains("\nStale Closed Queue Labels\n- none\n"));
	assert!(rendered.contains("\nRecovery Worktrees\n- none\n"));
	assert!(rendered.contains("\nPost-Review Lanes\n- none\n"));
	assert!(!rendered.contains("Warning details:"));
	assert!(!rendered.contains("run_id:"));
	assert!(!rendered.contains("run_lease: true"));
	assert!(!rendered.contains("role: post_review_lane"));
	assert!(!rendered.contains("role: cleanup_only"));
}
