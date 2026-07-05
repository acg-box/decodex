use std::{process::Command, thread, time::Duration};

use crate::{
	orchestrator::{
		self, RetryQueue, StateStore, tests,
		tests::{FakeTracker, retry_scheduling::support},
	},
	worktree::WorktreeManager,
};

#[test]
fn exited_unsuccessful_child_does_not_downgrade_persisted_success() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let run_id = "planned-run";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "succeeded")
		.expect("run attempt should record completed child outcome");

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
		run_id: String::from(run_id),
		attempt_number: 1,
		initial_issue_state: issue.state.name.clone(),
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: orchestrator::IssueDispatchMode::Retry,
		from_retry_queue: false,
		workflow: workflow.clone(),
	}];
	let mut retry_queue = RetryQueue::default();

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
		retry_queue.entries.is_empty(),
		"persisted success should not schedule a retry after a late wrapper failure"
	);
	assert_eq!(
		state_store
			.run_attempt(run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should remain recorded")
			.status(),
		"succeeded"
	);
}
