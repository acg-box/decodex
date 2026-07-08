use crate::orchestrator::tests::{
	operator::status::{
		running_lanes,
		running_lanes::{FakeTracker, StateStore, fs, orchestrator, state},
	},
	recovery_terminal_support,
};

#[test]
fn runtime_recovery_splits_invalid_local_id_batch_without_losing_valid_issue() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let mut issue = recovery_terminal_support::sample_active_issue("In Progress");

	issue.id = String::from("00000000-0000-0000-0000-000000000101");

	let tracker = FakeTracker::with_refresh_error(
		vec![issue.clone()],
		"Linear GraphQL request failed: Argument Validation Error",
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("active worktree path should exist");
	state::write_run_activity_marker(&worktree_path, "run-101", 1)
		.expect("activity marker should write");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("valid worktree mapping should record");
	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("invalid local run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("invalid local lease should record");

	let recovered_state = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("runtime recovery should split invalid local ids from valid server ids");
	let recovered_mapping = state_store
		.worktree_for_issue(&issue.id)
		.expect("mapping lookup should succeed")
		.expect("valid issue mapping should remain");
	let lease = state_store
		.lease_for_issue(&issue.id)
		.expect("lease lookup should succeed")
		.expect("valid issue lease should recover");

	assert!(
		recovered_state.recoverable_issues.is_empty(),
		"fresh valid issue should recover as active lease rather than disappear"
	);
	assert_eq!(recovered_mapping.issue_id(), issue.id);
	assert_eq!(lease.issue_id(), issue.id);
	assert_eq!(lease.run_id(), "run-101");
}

#[test]
fn splits_invalid_local_id_batch_without_losing_valid() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let mut issue = recovery_terminal_support::sample_active_issue("In Review");

	issue.id = String::from("00000000-0000-0000-0000-000000000101");

	let tracker = FakeTracker::with_refresh_error(
		vec![issue.clone()],
		"Linear GraphQL request failed: Argument Validation Error",
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let valid_worktree_path = config.worktree_root().join(&issue.identifier);
	let missing_ghost_path = config.worktree_root().join("PUB-012");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&valid_worktree_path.display().to_string(),
		)
		.expect("valid worktree mapping should record");
	state_store
		.upsert_worktree(
			"pubfi",
			"PUB-012",
			"x/pubfi-pub-012",
			&missing_ghost_path.display().to_string(),
		)
		.expect("stale local-id worktree mapping should record");

	let worktree_issues =
		orchestrator::load_post_review_worktree_issues(&tracker, &config, &state_store)
			.expect("post-review refresh should split invalid local ids from valid server ids");
	let (worktree, refreshed_issue) =
		worktree_issues.first().expect("valid post-review worktree issue should remain");

	assert_eq!(worktree_issues.len(), 1);
	assert_eq!(worktree.issue_id(), issue.id);
	assert_eq!(refreshed_issue.id, issue.id);
	assert!(
		tracker
			.refresh_queries
			.borrow()
			.iter()
			.any(|query| query == &vec![String::from("PUB-012")]),
		"stale local issue id should be retried in isolation"
	);
}
