use std::{process::Command, time::Duration};

use crate::orchestrator::{
	self, ChildExitRetryContext, ChildRunRef, IssueDispatchMode, RetryQueue, StateStore,
	tests::{self, FakeTracker, retry_scheduling::support},
};

#[test]
fn schedule_retry_after_child_exit_records_failure_retries_for_active_dispatch_modes() {
	for (issue_state, dispatch_mode, expected_dispatch_mode, run_id) in [
		("In Progress", IssueDispatchMode::Retry, IssueDispatchMode::Retry, "run-1"),
		(
			"In Review",
			IssueDispatchMode::ReviewRepair,
			IssueDispatchMode::ReviewRepair,
			"run-review-repair",
		),
	] {
		let (_temp_dir, config, workflow) = tests::temp_project_layout();
		let issue = support::sample_service_owned_issue(issue_state);
		let tracker =
			FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
		let state_store = StateStore::open_in_memory().expect("state store should open");

		state_store
			.record_run_attempt(run_id, &issue.id, 1, "failed")
			.expect("run attempt should record");

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
			ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
			issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
			&issue.state.name,
			dispatch_mode,
			exit_status,
		)
		.expect("failure retry should schedule");

		let entry =
			retry_queue.entries.get(&issue.id).expect("retry entry should exist for the issue");

		assert_eq!(entry.dispatch_mode, expected_dispatch_mode);
		assert_eq!(entry.kind, orchestrator::RetryKind::Failure);
		assert_eq!(entry.attempt, 1);
	}
}

#[test]
fn failure_retry_budget_ignores_prior_continuation_attempts() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-4";

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "succeeded")
		.expect("first continuation attempt should record");
	state_store
		.record_run_attempt("run-2", &issue.id, 2, "succeeded")
		.expect("second continuation attempt should record");
	state_store
		.record_run_attempt("run-3", &issue.id, 3, "succeeded")
		.expect("third continuation attempt should record");
	state_store
		.record_run_attempt(run_id, &issue.id, 4, "failed")
		.expect("first failure attempt should record");

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
		ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 4 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("first failure after continuations should still schedule");

	let entry = retry_queue.entries.get(&issue.id).expect("retry entry should exist for the issue");

	assert_eq!(entry.kind, orchestrator::RetryKind::Failure);
	assert_eq!(entry.attempt, 1);
	assert_eq!(
		orchestrator::retry_delay(entry.kind, entry.attempt, &workflow),
		Duration::from_millis(10_000)
	);
}
