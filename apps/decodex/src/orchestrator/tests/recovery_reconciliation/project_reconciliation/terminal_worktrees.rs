use crate::{
	orchestrator::{
		self, TERMINAL_GUARDED_RUN_STATUS,
		tests::{self, FakeTracker},
	},
	state::{ReviewPolicyCheckpointInput, StateStore},
	worktree::WorktreeManager,
};

#[test]
fn project_reconciliation_clears_terminal_identifier_worktree_before_tracker_refresh() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let tracker = FakeTracker::new(Vec::new());
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new("pubfi", config.repo_root(), config.worktree_root());
	let stale_issue_id = "PUB-001";
	let stale_worktree_path = config.worktree_root().join(stale_issue_id);

	state_store
		.record_run_attempt("run-01", stale_issue_id, 1, TERMINAL_GUARDED_RUN_STATUS)
		.expect("terminal run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			stale_issue_id,
			"x/pubfi-pub-001",
			&stale_worktree_path.display().to_string(),
		)
		.expect("stale worktree mapping should record");

	orchestrator::reconcile_project_state(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&worktree_manager,
	)
	.expect("project reconciliation should succeed");

	assert!(
		state_store
			.worktree_for_issue(stale_issue_id)
			.expect("worktree lookup should succeed")
			.is_none(),
		"terminal unleased identifier mapping should be cleared before tracker refresh"
	);
	assert!(
		tracker
			.refresh_queries
			.borrow()
			.iter()
			.flatten()
			.all(|issue_id| issue_id != stale_issue_id),
		"stale local identifier id must not be sent to tracker refresh"
	);
}

#[test]
fn project_reconciliation_preserves_terminal_identifier_worktree_with_review_authority() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let stale_issue_id = "PUB-001";
	let branch_name = "x/pubfi-pub-001";
	let issue = tests::sample_issue_with_sort_fields(
		stale_issue_id,
		stale_issue_id,
		"In Review",
		&[],
		Some(1),
		"2026-06-19T00:00:00.000Z",
	);
	let tracker = FakeTracker::new(vec![issue]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new("pubfi", config.repo_root(), config.worktree_root());
	let stale_worktree_path = config.worktree_root().join(stale_issue_id);

	state_store
		.record_run_attempt("run-01", stale_issue_id, 1, TERMINAL_GUARDED_RUN_STATUS)
		.expect("terminal run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			stale_issue_id,
			branch_name,
			&stale_worktree_path.display().to_string(),
		)
		.expect("stale worktree mapping should record");

	tests::seed_review_lifecycle_handoff_fixture(
		&state_store,
		config.service_id(),
		stale_issue_id,
		branch_name,
		"https://github.com/example/decodex/pull/1016",
		"head-oid",
	);

	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: config.service_id(),
			issue_id: stale_issue_id,
			run_id: "run-01",
			attempt_number: 1,
			phase: "handoff",
			review_level: "independent",
			status: "clean",
			head_sha: "head-oid",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("review checkpoint should record");

	orchestrator::reconcile_project_state(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&worktree_manager,
	)
	.expect("project reconciliation should preserve review authority");

	assert!(
		state_store
			.worktree_for_issue(stale_issue_id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"review-owned identifier mapping must not be cleared as stale residue"
	);
	assert!(
		state_store
			.review_lifecycle_handoff_fixture(config.service_id(), stale_issue_id, branch_name)
			.expect("review handoff lookup should succeed")
			.is_some(),
		"review lifecycle authority must be preserved"
	);
	assert!(
		state_store
			.review_policy_checkpoint(config.service_id(), stale_issue_id, "run-01", 1, "handoff",)
			.expect("review checkpoint lookup should succeed")
			.is_some(),
		"review checkpoint authority must be preserved"
	);
}
