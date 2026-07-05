use std::{
	process::Command,
	thread,
	time::{Duration, Instant},
};

use crate::{
	orchestrator::{
		self, IssueDispatchMode, RetryEntry, RetryEntryLifecycle, RetryKind, RetryQueue,
		StateStore, tests,
		tests::{FakeTracker, retry_scheduling::support},
	},
	worktree::WorktreeManager,
};

#[test]
fn exited_retry_child_keeps_queued_claim_when_no_run_attempt_was_persisted() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let mut child =
		Command::new("sh").args(["-c", "exit 1"]).spawn().expect("child process should spawn");

	for _ in 0..20 {
		if child.try_wait().expect("child status should query").is_some() {
			break;
		}

		thread::sleep(Duration::from_millis(10));
	}

	let mut active_children = vec![orchestrator::DaemonRunChild {
		child,
		issue_id: issue.id.clone(),
		run_id: String::from("planned-run"),
		attempt_number: 1,
		initial_issue_state: issue.state.name.clone(),
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: orchestrator::IssueDispatchMode::Retry,
		from_retry_queue: true,
		workflow: workflow.clone(),
	}];
	let mut retry_queue = RetryQueue::default();

	retry_queue.upsert(RetryEntry {
		issue_id: issue.id.clone(),
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		continuation_initial_issue_state: None,
		lifecycle: RetryEntryLifecycle::Active,
		dispatch_mode: IssueDispatchMode::Retry,
		kind: RetryKind::Failure,
		attempt: 1,
		ready_at: Instant::now(),
	});

	orchestrator::inspect_or_clear_active_children(
		&mut active_children,
		&mut retry_queue,
		&tracker,
		&config,
		&workflow,
		&state_store,
		&worktree_manager,
	)
	.expect("exited child cleanup should succeed");

	assert!(active_children.is_empty(), "exited child should be cleared");
	assert!(
		retry_queue.entries.contains_key(&issue.id),
		"retry claim should remain queued when the child exits before persisting a run attempt"
	);
}
