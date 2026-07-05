use std::fs;

use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker, recovery_reconciliation::support},
	},
	state::{self, StateStore},
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
