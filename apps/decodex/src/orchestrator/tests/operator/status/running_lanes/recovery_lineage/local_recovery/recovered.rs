use crate::{
	lane_authority::{LaneCommand, LaneId},
	orchestrator::tests::{
		operator::status::{
			running_lanes,
			running_lanes::{FakeTracker, StateStore, fs, orchestrator, state},
		},
		recovery_terminal_support,
	},
};

#[test]
fn runtime_recovery_records_recovered_provenance_for_fresh_active_worktree() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Progress");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("active worktree path should exist");
	state::write_run_activity_marker(&worktree_path, "run-1", 1)
		.expect("activity marker should write");
	let binding = config.project_binding("test-config-fingerprint");
	let lane_id = LaneId::new(config.service_id(), &issue.id).expect("lane id");
	state_store
		.apply_lane_command(
			lane_id.clone(),
			binding.config_fingerprint(),
			LaneCommand::Admit { intake_authority_id: String::from("authority-1") },
		)
		.expect("admit lane");
	state_store
		.apply_lane_command(
			lane_id,
			binding.config_fingerprint(),
			LaneCommand::AcquireClaim { run_id: String::from("run-1") },
		)
		.expect("claim lane");

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("activity marker should load")
		.expect("activity marker should exist");
	let observed_at_unix =
		marker.last_activity_unix_epoch().expect("activity marker should have a stable timestamp");
	let recovered_state = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("runtime recovery should succeed");
	let mapping = state_store
		.worktree_for_issue(&issue.id)
		.expect("mapping lookup should succeed")
		.expect("recovered mapping should exist");
	let lane = state_store
		.lane(&LaneId::new(config.service_id(), &issue.id).expect("lane id"))
		.expect("lane lookup")
		.expect("canonical lane remains");
	let attempt = state_store
		.run_attempt("run-1")
		.expect("attempt lookup")
		.expect("recovered attempt");

	assert!(
		recovered_state.recoverable_issues.is_empty(),
		"fresh marker should recover as the run lease instead of a retry queue item"
	);
	assert_eq!(mapping.provenance().source(), "runtime_recovered");
	assert_eq!(mapping.provenance().created_at_unix(), Some(observed_at_unix));
	assert_eq!(mapping.provenance().updated_at_unix(), Some(observed_at_unix));
	assert_eq!(lane.claim_run_id(), Some("run-1"));
	assert_eq!(attempt.project_id(), Some(config.service_id()));
	assert_eq!(attempt.issue_id(), issue.id);
}

#[test]
fn runtime_recovery_refuses_marker_that_does_not_match_canonical_claim() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Progress");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store");
	let worktree_path = config.worktree_root().join(&issue.identifier);
	fs::create_dir_all(&worktree_path).expect("worktree path");
	state::write_run_activity_marker(&worktree_path, "marker-run", 1).expect("marker");
	let binding = config.project_binding("test-config-fingerprint");
	let lane_id = LaneId::new(config.service_id(), &issue.id).expect("lane id");
	state_store
		.apply_lane_command(
			lane_id.clone(),
			binding.config_fingerprint(),
			LaneCommand::Admit { intake_authority_id: String::from("authority-1") },
		)
		.expect("admit");
	state_store
		.apply_lane_command(
			lane_id,
			binding.config_fingerprint(),
			LaneCommand::AcquireClaim { run_id: String::from("canonical-run") },
		)
		.expect("claim");

	let error = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.err()
	.expect("mismatched marker must fail closed");
	assert!(error.to_string().contains("does not match canonical lane authority"));
	assert!(state_store.lease_for_issue(&issue.id).expect("lease read").is_none());
}
