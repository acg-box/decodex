use std::time::Instant;

use time::OffsetDateTime;

use crate::{
	orchestrator::{
		self, IssueDispatchMode, RetryEntry, RetryEntryLifecycle, RetryKind, RetryQueue,
		StateStore,
		tests::{self, FakeTracker, retry_selection},
	},
	state,
	workflow::WorkflowDocument,
};

#[test]
fn due_retry_claim_releases_when_issue_becomes_not_dispatchable() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = retry_selection::selection_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
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

	let decision = orchestrator::plan_due_retry_run(
		&mut retry_queue,
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("retry planning should succeed");

	assert!(matches!(decision, orchestrator::RetryDispatchDecision::Continue));
	assert!(retry_queue.is_empty(), "due not-dispatchable issue should release the queued claim");
}

#[test]
fn due_retry_claim_release_clears_persisted_retry_marker() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = retry_selection::selection_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("PUB-101");
	let mut retry_queue = RetryQueue::default();

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_retry_schedule(
		&worktree_path,
		"run-1",
		1,
		"failure",
		OffsetDateTime::now_utc().unix_timestamp(),
	)
	.expect("retry schedule should write");

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

	let decision = orchestrator::plan_due_retry_run(
		&mut retry_queue,
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("retry planning should succeed");

	assert!(matches!(decision, orchestrator::RetryDispatchDecision::Continue));
	assert!(
		retry_queue.is_empty(),
		"not-dispatchable issue should release the queued claim when due"
	);

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("marker should load")
		.expect("marker should still exist");

	assert_eq!(marker.retry_kind(), None);
	assert_eq!(marker.retry_ready_at_unix_epoch(), None);
}

#[test]
fn due_continuation_retry_dispatches_when_issue_still_reflects_startable_state() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = retry_selection::selection_sample_service_owned_issue("Todo");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut retry_queue = RetryQueue::default();

	retry_queue.upsert(RetryEntry {
		issue_id: issue.id.clone(),
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		continuation_initial_issue_state: Some(issue.state.name.clone()),
		lifecycle: RetryEntryLifecycle::Active,
		dispatch_mode: IssueDispatchMode::Retry,
		kind: RetryKind::Continuation,
		attempt: 1,
		ready_at: Instant::now(),
	});

	let next_run = orchestrator::plan_next_daemon_run(
		&mut retry_queue,
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("daemon planning should succeed")
	.expect("the continuation retry should still dispatch");

	assert!(next_run.1, "continuation work should still come from the retry queue");
	assert_eq!(next_run.0.issue_id, issue.id);
	assert_eq!(next_run.0.issue_identifier, issue.identifier);
	assert_eq!(next_run.0.issue_state, "In Progress");
	assert_eq!(next_run.0.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
}

#[test]
fn due_continuation_retry_releases_when_issue_moves_to_different_startable_state() {
	let workflow = WorkflowDocument::parse_markdown(
		&tests::sample_workflow_markdown("pubfi", &[], "Continuation retry policy.\n", 1)
			.replace("startable_states = [\"Todo\"]", "startable_states = [\"Todo\", \"Backlog\"]"),
	)
	.expect("workflow should parse");
	let (_temp_dir, config, _default_workflow) = tests::temp_project_layout();
	let issue = retry_selection::selection_sample_service_owned_issue("Backlog");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut retry_queue = RetryQueue::default();

	retry_queue.upsert(RetryEntry {
		issue_id: issue.id.clone(),
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		continuation_initial_issue_state: Some(String::from("Todo")),
		lifecycle: RetryEntryLifecycle::Active,
		dispatch_mode: IssueDispatchMode::Retry,
		kind: RetryKind::Continuation,
		attempt: 1,
		ready_at: Instant::now(),
	});

	let decision = orchestrator::plan_due_retry_run(
		&mut retry_queue,
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("retry planning should succeed");

	assert!(matches!(decision, orchestrator::RetryDispatchDecision::Continue));
	assert!(
		retry_queue.is_empty(),
		"continuation retention should reject a different startable state instead of reopening the old thread"
	);
}
