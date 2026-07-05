use crate::orchestrator::tests::operator::status::running_lanes::{
	self, StateStore, TEST_SERVICE_ID, lifecycle::attempt_history, orchestrator, tracker,
};

#[test]
fn operator_status_supersedes_stale_repair_findings_after_clean_handoff_checkpoint() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let run_id = "run-review";
	let repair_head = "1111111111111111111111111111111111111111";
	let clean_head = "2222222222222222222222222222222222222222";

	state_store
		.record_run_attempt(run_id, &issue.id, 2, "running")
		.expect("review attempt should record");
	state_store
		.upsert_lease(TEST_SERVICE_ID, &issue.id, run_id, "In Progress")
		.expect("lease should record");

	let stale_repair_next_action = attempt_history::seed_stale_repair_and_clean_handoff_checkpoints(
		&state_store,
		&config,
		&issue.id,
		run_id,
		repair_head,
		clean_head,
	);
	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 0)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let loop_status = run.loop_status.as_ref().expect("loop status should render");
	let review = loop_status.review.as_ref().expect("review status should render");
	let checkpoint = review.checkpoint.as_ref().expect("review checkpoint should render");

	assert_eq!(review.phase, "handoff");
	assert_eq!(review.status, "clean");
	assert_eq!(checkpoint.head_sha, clean_head);
	assert!(checkpoint.active_fingerprints.is_empty());
	assert_eq!(run.policy_state, "allowed");
	assert_eq!(
		run.lane_control_next_action,
		"Push or update the PR and record review handoff for the clean current lane head."
	);
	assert_ne!(loop_status.next_action.as_deref(), Some(stale_repair_next_action));
}
