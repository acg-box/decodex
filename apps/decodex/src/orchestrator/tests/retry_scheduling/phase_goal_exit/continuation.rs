use std::process::Command;

use crate::orchestrator::{
	self, CONTINUATION_PENDING_RUN_STATUS, ChildExitRetryContext, ChildRunRef, IssueDispatchMode,
	RetryQueue, StateStore,
	tests::{self, FakeTracker, TEST_SERVICE_ID, retry_scheduling::support},
};

#[test]
fn schedule_retry_after_child_exit_records_continuation_retry_for_clean_exit() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-1";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, CONTINUATION_PENDING_RUN_STATUS)
		.expect("run attempt should record");

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
	.expect("continuation retry should schedule");

	let entry = retry_queue.entries.get(&issue.id).expect("retry entry should exist for the issue");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, run_id, 1)
		.expect("private continuation lineage events should load");

	assert_eq!(entry.kind, orchestrator::RetryKind::Continuation);
	assert_eq!(entry.attempt, 1);
	assert!(events.iter().any(|event| {
		event.event_type() == "continuation_lineage"
			&& event.payload()["continuation_of_run_id"] == run_id
			&& event.payload()["retry_budget_consumed"] == false
			&& event.payload()["next_retry_kind"] == "continuation"
	}));
}

#[test]
fn schedule_retry_after_child_exit_retains_continuation_retry_for_stale_startable_issue() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::sample_service_owned_issue("Todo");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-1";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, CONTINUATION_PENDING_RUN_STATUS)
		.expect("run attempt should record");

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
	.expect("continuation retry should tolerate a stale startable tracker reread");

	let entry = retry_queue.entries.get(&issue.id).expect("retry entry should exist for the issue");

	assert_eq!(entry.kind, orchestrator::RetryKind::Continuation);
	assert_eq!(entry.attempt, 1);
}
