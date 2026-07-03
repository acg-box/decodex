use std::{
	fs,
	time::{Duration, Instant},
};

use time::OffsetDateTime;

use crate::{
	orchestrator::{
		self, IssueDispatchMode, RetryDispatchDecision, RetryEntry, RetryEntryLifecycle, RetryKind,
		RetryQueue, StateStore, tests,
		tests::{FakeTracker, TEST_SERVICE_ID},
	},
	state,
	tracker::{self, TrackerIssue},
	workflow::WorkflowDocument,
	worktree::WorktreeManager,
};

fn selection_sample_service_owned_issue(state_name: &str) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	tests::sample_issue(state_name, &[active_label.as_str()])
}

fn selection_sample_service_owned_issue_with_sort_fields(
	id: &str,
	identifier: &str,
	state_name: &str,
	sort_value: Option<i64>,
	updated_at: &str,
) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	tests::sample_issue_with_sort_fields(
		id,
		identifier,
		state_name,
		&[active_label.as_str()],
		sort_value,
		updated_at,
	)
}

fn selection_sample_service_owned_issue_with_project_slug_and_sort_fields(
	id: &str,
	identifier: &str,
	project_slug: &str,
	state_name: &str,
	sort_value: Option<i64>,
	updated_at: &str,
) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	tests::sample_issue_with_project_slug_and_sort_fields(
		id,
		identifier,
		project_slug,
		state_name,
		&[active_label.as_str()],
		sort_value,
		updated_at,
	)
}

#[test]
fn queued_retry_blocks_normal_candidate_selection_until_due() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = selection_sample_service_owned_issue("In Progress");
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
	let issue = selection_sample_service_owned_issue("In Progress");
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
	let first_future_retry = selection_sample_service_owned_issue_with_sort_fields(
		"issue-1",
		"PUB-101",
		"In Progress",
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let second_future_retry = selection_sample_service_owned_issue_with_sort_fields(
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
	let issue = selection_sample_service_owned_issue_with_project_slug_and_sort_fields(
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
	let issue = selection_sample_service_owned_issue("In Review");
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

#[test]
fn due_retry_claim_releases_when_issue_becomes_not_dispatchable() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = selection_sample_service_owned_issue("In Review");
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
	let issue = selection_sample_service_owned_issue("In Review");
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
	let issue = selection_sample_service_owned_issue("Todo");
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
	let issue = selection_sample_service_owned_issue("Backlog");
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

#[test]
fn future_retry_claim_stays_blocked_when_issue_returns_to_todo_before_due_time() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = selection_sample_service_owned_issue("Todo");
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

#[test]
fn due_retry_dispatches_when_another_issue_has_active_lease() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = selection_sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut retry_queue = RetryQueue::default();

	state_store
		.upsert_lease("pubfi", "issue-other", "run-other", "In Progress")
		.expect("temporary competing lease should record");
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

	assert!(matches!(
		decision,
		RetryDispatchDecision::Dispatch(summary) if summary.issue_id == issue.id
	));
}

#[test]
fn due_retry_claim_stays_queued_when_issue_is_claimed_by_another_process() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = selection_sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let local_store = StateStore::open_in_memory().expect("local state store should open");
	let remote_store = StateStore::open_in_memory().expect("remote state store should open");
	let mut retry_queue = RetryQueue::default();

	local_store
		.configure_dispatch_slot_root(config.service_id(), config.worktree_root())
		.expect("local dispatch slot root should configure");
	remote_store
		.configure_dispatch_slot_root(config.service_id(), config.worktree_root())
		.expect("remote dispatch slot root should configure");

	assert!(
		remote_store
			.try_acquire_lease(config.service_id(), &issue.id, "run-foreign", "In Progress")
			.expect("foreign process should acquire the shared issue claim")
	);

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
		&local_store,
	)
	.expect("retry planning should succeed");

	assert!(matches!(
		decision,
		RetryDispatchDecision::Blocked{ excluded_issue_ids }
			if excluded_issue_ids == vec![issue.id.clone()]
	));
	assert!(
		retry_queue.entries.contains_key(&issue.id),
		"retry queue should keep the claim until the foreign issue claim clears"
	);
}

#[test]
fn due_closeout_retry_stays_queued_when_pr_state_read_fails() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(
		&base_config,
		"DECODEX_TEST_MISSING_DELIVERY_CLOSEOUT_GITHUB_TOKEN",
	);
	let issue = selection_sample_service_owned_issue("Done");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/178";
	let mut retry_queue = RetryQueue::default();

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	retry_queue.upsert(RetryEntry {
		issue_id: issue.id.clone(),
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		continuation_initial_issue_state: Some(String::from("In Review")),
		lifecycle: RetryEntryLifecycle::Closeout,
		dispatch_mode: IssueDispatchMode::Closeout,
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
	.expect("retry planning should degrade GH read failures to a blocked queued retry");

	assert!(matches!(
		decision,
		RetryDispatchDecision::Blocked{ excluded_issue_ids }
			if excluded_issue_ids == vec![issue.id.clone()]
	));
	assert!(
		retry_queue.entries.contains_key(&issue.id),
		"completed closeout retries must remain queued when GH state inspection is temporarily unavailable"
	);
}
