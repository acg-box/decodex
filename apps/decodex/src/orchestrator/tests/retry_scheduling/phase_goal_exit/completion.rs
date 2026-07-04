use std::process::Command;

use crate::orchestrator::{
	self, ChildExitRetryContext, ChildRunRef, IssueDispatchMode, RetryQueue, StateStore,
	tests::{self, FakeTracker, retry_scheduling::support},
};

#[test]
fn schedule_retry_after_child_exit_skips_retry_for_completed_successful_run() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-1";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "succeeded")
		.expect("completed run attempt should record");

	let exit_status =
		Command::new("sh").args(["-c", "exit 0"]).status().expect("success exit should run");
	let mut retry_queue = RetryQueue::default();

	orchestrator::schedule_retry_after_child_exit(
		ChildExitRetryContext {
			retry_queue: &mut retry_queue,
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
		},
		ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("completed successful runs should not schedule another retry");

	assert!(
		!retry_queue.entries.contains_key(&issue.id),
		"successful review-handoff style exits must not reopen the same run as a continuation"
	);
}

#[test]
fn schedule_retry_after_child_exit_requires_exact_run_id() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.record_run_attempt("other-run", &issue.id, 1, "running")
		.expect("other run attempt should record");

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
		ChildRunRef { issue_id: &issue.id, run_id: "planned-run", attempt_number: 1 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("retry scheduling should succeed");

	assert!(
		!retry_queue.entries.contains_key(&issue.id),
		"retry scheduling should ignore a different run that only matches the issue and attempt"
	);
}
