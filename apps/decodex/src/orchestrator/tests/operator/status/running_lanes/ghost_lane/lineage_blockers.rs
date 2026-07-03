use crate::{
	orchestrator::tests::operator::status::{
		running_lanes,
		running_lanes::{
			FakeTracker, LinearExecutionEventIdentity, ReviewPolicyCheckpointInput, StateStore,
			TEST_SERVICE_ID, orchestrator,
		},
	},
	tracker::records::LinearExecutionEventRecord,
};

#[test]
fn live_operator_status_blocks_missing_issue_ghost_cleanup_when_review_checkpoint_exists() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: TEST_SERVICE_ID,
			issue_id: "PUB-012",
			run_id: "run-12",
			attempt_number: 1,
			phase: "handoff",
			review_level: config.codex().review_level().as_str(),
			status: "clean",
			head_sha: "2222222222222222222222222222222222222222",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("review checkpoint should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should be visible");

	assert_eq!(run.ownership_state, "retained_attention");
	assert_eq!(run.policy_state, "runtime_recovery_blocked");
	assert!(
		run.lane_control_conditions.contains(&String::from("review_policy_checkpoint_present"))
	);
}

#[test]
fn live_operator_status_blocks_missing_issue_ghost_cleanup_when_pr_lineage_exists() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let mut event = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "pubfi",
			issue_id: "PUB-012",
			issue_identifier: "PUB-012",
			run_id: "run-12",
			attempt_number: 1,
		},
		"closeout",
		String::from("2026-06-18T00:00:00Z"),
		"closeout",
	);

	event.branch = Some(String::from("x/pubfi-pub-012"));
	event.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/12"));
	event.pr_head_sha = Some(String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"));
	event.pr_base_ref = Some(String::from("main"));
	event.commit_sha = Some(String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d7"));
	event.summary = Some(String::from("Recorded retained closeout."));

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store.record_linear_execution_event(&event).expect("linear event should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should be visible");

	assert_eq!(run.ownership_state, "retained_attention");
	assert_eq!(run.policy_state, "runtime_recovery_blocked");
	assert!(run.lane_control_conditions.contains(&String::from("pr_or_review_lineage_present")));
}
