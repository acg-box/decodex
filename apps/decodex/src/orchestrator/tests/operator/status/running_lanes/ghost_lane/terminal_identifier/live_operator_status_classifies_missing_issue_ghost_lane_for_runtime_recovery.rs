use crate::orchestrator::tests::operator::status::running_lanes::{
	self, FakeTracker, StateStore, orchestrator,
};

#[test]
fn live_operator_status_classifies_missing_issue_ghost_lane_for_runtime_recovery() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("ghost current lane should be visible");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(run.run_id, "run-12");
	assert_eq!(run.issue_id, "PUB-012");
	assert_eq!(run.issue_identifier.as_deref(), Some("PUB-012"));
	assert_eq!(run.ownership_state, "ghost_lane");
	assert_eq!(run.policy_state, "runtime_recovery_required");
	assert_eq!(run.lane_control_next_action, "run_ghost_lane_recovery");
	assert!(!run.counts_as_running);
	assert!(run.needs_attention);
	assert!(run.lane_control_conditions.contains(&String::from("tracker_issue_missing")));
	assert!(run.lane_control_conditions.contains(&String::from("worktree_missing")));
	assert!(run.lane_control_conditions.contains(&String::from("control_channel_missing")));
	assert!(run.lane_control_conditions.contains(&String::from("private_evidence_missing")));
	assert!(run.lane_control_conditions.contains(&String::from("review_lineage_missing")));
	assert_eq!(project.attention_count, 1);
	assert!(!rendered.contains("Record the independent Decodex Review checkpoint"));
	assert!(!rendered.contains("review-handoff"));
}
