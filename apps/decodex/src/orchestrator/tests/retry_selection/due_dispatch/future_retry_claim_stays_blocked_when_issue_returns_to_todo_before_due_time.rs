use std::time::{Duration, Instant};

use crate::orchestrator::{
	self, IssueDispatchMode, RetryDispatchDecision, RetryEntry, RetryEntryLifecycle, RetryKind,
	RetryQueue, StateStore,
	tests::{self, FakeTracker, retry_selection},
};

#[test]
fn future_retry_claim_stays_blocked_when_issue_returns_to_todo_before_due_time() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = retry_selection::selection_sample_service_owned_issue("Todo");
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
