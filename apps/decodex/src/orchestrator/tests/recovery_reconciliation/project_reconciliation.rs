use std::{fs, time::Duration};

use crate::{
	agent::RUN_LEASE_IDLE_TIMEOUT,
	orchestrator::{
		self, RunLeaseDisposition, RunLeaseReconciliation, TERMINAL_GUARDED_RUN_STATUS,
		tests::{self, FakeTracker, recovery_reconciliation::support},
	},
	state::{self, ReviewPolicyCheckpointInput, StateStore},
	worktree::WorktreeManager,
};

#[test]
fn project_reconciliation_schedules_retry_for_orphaned_active_worktree_run() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::reconciliation_sample_service_owned_issue("In Progress");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new("pubfi", config.repo_root(), config.worktree_root());
	let run_id = "run-orphaned-active";
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	state::write_run_activity_marker_for_process(&worktree_path, run_id, 1, u32::MAX)
		.expect("stopped process marker should write");
	orchestrator::reconcile_project_state(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&worktree_manager,
	)
	.expect("project reconciliation should succeed");

	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none());
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"orphaned active worktree must stay available for operator recovery"
	);
	assert_eq!(
		state_store
			.run_attempt(run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should exist")
			.status(),
		"stalled"
	);
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("stalled_run_detected")
			&& comment.contains("decodex run failed and will retry")
			&& comment.contains("run-orphaned-active")
	}));

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("retry marker should load")
		.expect("retry marker should exist");

	assert_eq!(marker.retry_kind(), Some("failure"));
	assert!(marker.retry_ready_at_unix_epoch().is_some());
}

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

	tests::seed_review_handoff_marker(
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
			.review_handoff_marker(config.service_id(), stale_issue_id, branch_name)
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

#[test]
fn project_reconciliation_marks_orphaned_attention_worktree_run_stalled_without_tracker_writes() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &["decodex:needs-attention"]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new("pubfi", config.repo_root(), config.worktree_root());
	let run_id = "run-attention-orphan";
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	state::write_run_activity_marker_for_process(&worktree_path, run_id, 1, u32::MAX)
		.expect("stopped process marker should write");
	orchestrator::reconcile_project_state(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&worktree_manager,
	)
	.expect("project reconciliation should succeed");

	assert_eq!(
		state_store
			.run_attempt(run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should exist")
			.status(),
		"stalled"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"attention worktree must stay available for operator recovery"
	);
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
	assert!(tracker.comments.borrow().is_empty());
}

#[test]
fn stalled_run_reconciliation_preserves_retry_budget_marker_from_retained_worktree() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new("pubfi", config.repo_root(), config.worktree_root());
	let issue = tests::sample_issue("In Progress", &[]);
	let run_id = "run-stalled-budget";
	let worktree_path = config.worktree_root().join("PUB-101");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_retry_budget_attempt_count(&worktree_path, "older-run", 2, 2)
		.expect("retry budget marker should write");

	state_store
		.record_run_attempt(run_id, &issue.id, 3, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, run_id, "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let action = RunLeaseReconciliation {
		issue: issue.clone(),
		run_attempt: state_store
			.run_attempt(run_id)
			.expect("run attempt query should succeed")
			.expect("run attempt should exist"),
		worktree_mapping: state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree query should succeed"),
		disposition: RunLeaseDisposition::Stalled {
			idle_for: RUN_LEASE_IDLE_TIMEOUT + Duration::from_secs(1),
		},
		workflow: workflow.clone(),
	};

	orchestrator::apply_run_lease_reconciliation(
		&tracker,
		&config,
		&state_store,
		&worktree_manager,
		vec![action],
	)
	.expect("reconciliation should succeed");

	assert_eq!(
		state::read_run_retry_budget_attempt_count(&worktree_path)
			.expect("retry budget marker should read")
			.expect("retry budget marker should remain present"),
		2,
		"stalled reconciliation should preserve the retained retry-budget base"
	);
}
