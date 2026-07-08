use std::time::{Duration, Instant};

use crate::orchestrator::{
	self, IssueDispatchMode, RetryDispatchDecision, RetryEntry, RetryEntryLifecycle, RetryKind,
	RetryQueue, StateStore,
	tests::{self, FakeTracker, retry_selection},
};

#[test]
fn blocked_when_issue_moves_project() {
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
