use std::{
	fs,
	time::{Duration, Instant},
};

use crate::{
	orchestrator::{
		self, IssueDispatchMode, RetryDispatchDecision, RetryEntry, RetryEntryLifecycle, RetryKind,
		RetryQueue, StateStore,
		tests::{self, FakeTracker, retry_selection},
	},
	workflow::WorkflowDocument,
	worktree::WorktreeManager,
};

#[test]
fn queued_retry_blocks_normal_candidate_selection_until_due() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = retry_selection::selection_sample_service_owned_issue("In Progress");
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
		attempt: 2,
		ready_at: Instant::now() + Duration::from_secs(60),
	});

	let decision = orchestrator::plan_due_retry_run(
		&mut retry_queue,
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("retry planning should succeed");

	assert!(matches!(
		decision,
		RetryDispatchDecision::Blocked{ excluded_issue_ids }
			if excluded_issue_ids == vec![issue.id.clone()]
	));
	assert!(!retry_queue.is_empty(), "future retry should keep the queued claim");
	assert_eq!(
		tracker.refresh_snapshots.borrow().len(),
		1,
		"future retry planning should not refresh tracker state before the retry is due"
	);
}

#[test]
fn queued_retry_stays_blocked_when_project_lookup_blips_before_due_time() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = retry_selection::selection_sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]])
			.with_project_lookup_error("transient project lookup failure");
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
		attempt: 2,
		ready_at: Instant::now() + Duration::from_secs(60),
	});

	let decision = orchestrator::plan_due_retry_run(
		&mut retry_queue,
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("future retry should not fail on project lookup blips");

	assert!(matches!(
		decision,
		RetryDispatchDecision::Blocked{ excluded_issue_ids }
			if excluded_issue_ids == vec![issue.id.clone()]
	));
	assert!(!retry_queue.is_empty(), "future retry should keep the queued claim");
	assert_eq!(
		tracker.refresh_snapshots.borrow().len(),
		1,
		"future retry planning should not refresh tracker state before the retry is due"
	);
}

#[test]
fn blocked_future_retry_excludes_all_queued_retries_before_normal_fallback() {
	let workflow = WorkflowDocument::parse_markdown(&tests::sample_workflow_markdown(
		"pubfi",
		&[],
		"Retry exclusion policy.\n",
		1,
	))
	.expect("workflow should parse");
	let first_future_retry = retry_selection::selection_sample_service_owned_issue_with_sort_fields(
		"issue-1",
		"PUB-101",
		"In Progress",
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let second_future_retry =
		retry_selection::selection_sample_service_owned_issue_with_sort_fields(
			"issue-2",
			"PUB-102",
			"In Progress",
			Some(2),
			"2026-03-13T04:17:17.133Z",
		);
	let todo_issue = tests::sample_issue_with_sort_fields(
		"issue-3",
		"PUB-103",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:18:17.133Z",
	);
	let listed_issues =
		vec![first_future_retry.clone(), second_future_retry.clone(), todo_issue.clone()];
	let tracker = FakeTracker::with_refresh_snapshots(
		listed_issues.clone(),
		vec![listed_issues.clone(), listed_issues.clone(), listed_issues],
	);
	let (_temp_dir, config, _default_workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let retained_worktree = worktree_manager.plan_for_issue(&second_future_retry.identifier);

	fs::create_dir_all(&retained_worktree.path)
		.expect("retained future retry worktree should exist for recovery");

	let mut retry_queue = RetryQueue::default();

	retry_queue.upsert(RetryEntry {
		issue_id: first_future_retry.id.clone(),
		retry_project_slug: first_future_retry
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		continuation_initial_issue_state: None,
		lifecycle: RetryEntryLifecycle::Active,
		dispatch_mode: IssueDispatchMode::Retry,
		kind: RetryKind::Failure,
		attempt: 2,
		ready_at: Instant::now() + Duration::from_secs(60),
	});
	retry_queue.upsert(RetryEntry {
		issue_id: second_future_retry.id.clone(),
		retry_project_slug: second_future_retry
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		continuation_initial_issue_state: None,
		lifecycle: RetryEntryLifecycle::Active,
		dispatch_mode: IssueDispatchMode::Retry,
		kind: RetryKind::Failure,
		attempt: 2,
		ready_at: Instant::now() + Duration::from_secs(120),
	});

	let next_run = orchestrator::plan_next_daemon_run(
		&mut retry_queue,
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("daemon planning should succeed")
	.expect("normal work should still dispatch");

	assert!(!next_run.1, "alternate work should not dispatch from the retry queue");
	assert_eq!(next_run.0.issue_id, todo_issue.id);
	assert_eq!(next_run.0.issue_identifier, todo_issue.identifier);
	assert_eq!(next_run.0.issue_state, "In Progress");
	assert_eq!(next_run.0.dispatch_mode, orchestrator::IssueDispatchMode::Normal);
	assert!(
		retry_queue.entries.contains_key(&first_future_retry.id)
			&& retry_queue.entries.contains_key(&second_future_retry.id),
		"all queued retries should stay queued instead of bypassing their ready_at through retained recovery"
	);
}

#[test]
fn future_retry_claim_stays_blocked_when_issue_moves_to_another_project_before_due_time() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue =
		retry_selection::selection_sample_service_owned_issue_with_project_slug_and_sort_fields(
			"issue-1",
			"PUB-101",
			"other-project",
			"In Progress",
			Some(3),
			"2026-03-13T04:16:17.133Z",
		);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut retry_queue = RetryQueue::default();

	retry_queue.upsert(RetryEntry {
		issue_id: issue.id.clone(),
		retry_project_slug: String::from("pubfi"),
		continuation_initial_issue_state: None,
		lifecycle: RetryEntryLifecycle::Active,
		dispatch_mode: IssueDispatchMode::Retry,
		kind: RetryKind::Failure,
		attempt: 2,
		ready_at: Instant::now() + Duration::from_secs(60),
	});

	let decision = orchestrator::plan_due_retry_run(
		&mut retry_queue,
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("retry planning should keep queued claims before their due time");

	assert!(matches!(
		decision,
		RetryDispatchDecision::Blocked{ excluded_issue_ids }
			if excluded_issue_ids == vec![issue.id.clone()]
	));
	assert!(
		retry_queue.entries.contains_key(&issue.id),
		"future retries should keep their queued claim until due when the issue is still active"
	);
	assert_eq!(
		tracker.refresh_snapshots.borrow().len(),
		1,
		"future retry planning should not refresh tracker state before the retry is due"
	);
}

#[test]
fn future_retry_claim_stays_blocked_when_issue_becomes_not_dispatchable_before_due_time() {
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
		ready_at: Instant::now() + Duration::from_secs(60),
	});

	let decision = orchestrator::plan_due_retry_run(
		&mut retry_queue,
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("retry planning should succeed");

	assert!(matches!(
		decision,
		RetryDispatchDecision::Blocked{ excluded_issue_ids }
			if excluded_issue_ids == vec![issue.id.clone()]
	));
	assert!(
		retry_queue.entries.contains_key(&issue.id),
		"future retry should keep the local claim until due instead of polling remote state"
	);
	assert_eq!(
		tracker.refresh_snapshots.borrow().len(),
		1,
		"future retry planning should not refresh tracker state before the retry is due"
	);
}
