use std::process::Command;

use crate::{
	orchestrator::{
		self, ChildExitRetryContext, ChildRunRef, IssueDispatchMode, RetryQueue, StateStore, tests,
		tests::{FakeTracker, retry_scheduling::support},
	},
	state,
	worktree::WorktreeManager,
};

#[test]
fn schedule_retry_after_child_exit_terminalizes_exhausted_review_repair_issue() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	for attempt in 1..=3 {
		state_store
			.record_run_attempt(
				&format!("run-review-repair-{attempt}"),
				&issue.id,
				attempt,
				"failed",
			)
			.expect("failed repair attempt should record");
	}

	let exit_status =
		Command::new("sh").args(["-c", "exit 1"]).status().expect("failure exit should run");
	let mut retry_queue = RetryQueue::default();

	orchestrator::schedule_retry_after_child_exit(
		ChildExitRetryContext {
			retry_queue: &mut retry_queue,
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
		},
		ChildRunRef { issue_id: &issue.id, run_id: "run-review-repair-3", attempt_number: 3 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::ReviewRepair,
		exit_status,
	)
	.expect("exhausted review-repair child exit should terminalize");

	assert!(retry_queue.entries.is_empty(), "exhausted repair should not stay queued");
	assert_eq!(
		tracker.state_updates.borrow().as_slice(),
		&[(issue.id.clone(), String::from("state-todo"))]
	);
	assert_eq!(
		tracker.label_additions.borrow().as_slice(),
		&[(issue.id.clone(), vec![String::from("label-needs-attention")])]
	);
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		&[(issue.id.clone(), vec![String::from("label-active")])]
	);
	assert!(
		tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and needs attention")),
		"terminal failure comment should explain the exhausted repair"
	);
}

#[test]
fn schedule_retry_after_child_exit_counts_persisted_retry_budget_after_restart() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_retry_budget_attempt_count(&worktree.path, "previous-run", 2, 2)
		.expect("persisted retry budget marker should write");

	state_store
		.record_run_attempt("run-review-repair-3", &issue.id, 3, "failed")
		.expect("current failed repair attempt should record");

	let exit_status =
		Command::new("sh").args(["-c", "exit 1"]).status().expect("failure exit should run");
	let mut retry_queue = RetryQueue::default();

	orchestrator::schedule_retry_after_child_exit(
		ChildExitRetryContext {
			retry_queue: &mut retry_queue,
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
		},
		ChildRunRef { issue_id: &issue.id, run_id: "run-review-repair-3", attempt_number: 3 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::ReviewRepair,
		exit_status,
	)
	.expect("persisted retry budget should contribute to child-exit terminalization");

	assert!(retry_queue.entries.is_empty(), "exhausted repair should not stay queued");
	assert_eq!(
		tracker.state_updates.borrow().as_slice(),
		&[(issue.id.clone(), String::from("state-todo"))]
	);
	assert_eq!(
		tracker.label_additions.borrow().as_slice(),
		&[(issue.id.clone(), vec![String::from("label-needs-attention")])]
	);
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		&[(issue.id.clone(), vec![String::from("label-active")])]
	);
}
