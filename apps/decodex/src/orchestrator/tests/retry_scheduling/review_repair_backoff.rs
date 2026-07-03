use std::{
	process::Command,
	time::{Duration, Instant},
};

use crate::orchestrator::{
	self, ChildExitRetryContext, ChildRunRef, IssueDispatchMode, RetryDispatchDecision, RetryEntry,
	RetryEntryLifecycle, RetryKind, RetryQueue, StateStore,
	tests::{self, FakeTracker, retry_scheduling::support},
};

#[test]
fn future_review_repair_retry_keeps_backoff_window_until_due() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::sample_service_owned_issue("In Review");
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
		lifecycle: RetryEntryLifecycle::ReviewRepair,
		dispatch_mode: IssueDispatchMode::ReviewRepair,
		kind: RetryKind::Failure,
		attempt: 1,
		ready_at: Instant::now() + Duration::from_secs(60),
	});

	let decision = orchestrator::plan_due_retry_run(
		&mut retry_queue,
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("future review-repair retry should stay queued");

	assert!(matches!(
		decision,
		RetryDispatchDecision::Blocked{ excluded_issue_ids }
			if excluded_issue_ids == vec![issue.id.clone()]
	));
	assert!(
		retry_queue.entries.contains_key(&issue.id),
		"review-repair retries should keep their queued backoff window until ready"
	);
	assert_eq!(
		tracker.refresh_snapshots.borrow().len(),
		1,
		"future review-repair retry planning should not refresh tracker state before the retry is due"
	);
}

#[test]
fn due_review_repair_retry_drops_after_backoff_budget_exhausted() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut retry_queue = RetryQueue::default();

	for attempt in 1..=3 {
		state_store
			.record_run_attempt(&format!("run-{attempt}"), &issue.id, attempt, "failed")
			.expect("failed repair attempt should record");
	}

	retry_queue.upsert(RetryEntry {
		issue_id: issue.id.clone(),
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		continuation_initial_issue_state: None,
		lifecycle: RetryEntryLifecycle::ReviewRepair,
		dispatch_mode: IssueDispatchMode::ReviewRepair,
		kind: RetryKind::Failure,
		attempt: 3,
		ready_at: Instant::now(),
	});

	let decision = orchestrator::plan_due_retry_run(
		&mut retry_queue,
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("exhausted review-repair retry should be dropped");

	assert!(matches!(decision, RetryDispatchDecision::Continue));
	assert!(
		retry_queue.entries.is_empty(),
		"exhausted review-repair retry should not hold the queued claim"
	);
}

#[test]
fn due_review_repair_retry_drops_when_active_ownership_is_gone() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
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
		lifecycle: RetryEntryLifecycle::ReviewRepair,
		dispatch_mode: IssueDispatchMode::ReviewRepair,
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
	.expect("review-repair retry planning should succeed");

	assert!(matches!(decision, RetryDispatchDecision::Continue));
	assert!(
		!retry_queue.entries.contains_key(&issue.id),
		"review-repair retries should be dropped when active ownership is gone"
	);
}

#[test]
fn interrupted_exits_consume_retry_budget() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-3";

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "interrupted")
		.expect("first interrupted attempt should record");
	state_store
		.record_run_attempt("run-2", &issue.id, 2, "interrupted")
		.expect("second interrupted attempt should record");
	state_store
		.record_run_attempt(run_id, &issue.id, 3, "interrupted")
		.expect("third interrupted attempt should record");

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
		ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 3 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("retry scheduling should succeed");

	assert!(
		!retry_queue.entries.contains_key(&issue.id),
		"interrupted exits should exhaust the retry budget"
	);
}
