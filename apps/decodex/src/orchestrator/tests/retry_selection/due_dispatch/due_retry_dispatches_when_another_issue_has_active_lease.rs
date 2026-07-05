use std::time::Instant;

use crate::orchestrator::{
	self, IssueDispatchMode, RetryDispatchDecision, RetryEntry, RetryEntryLifecycle, RetryKind,
	RetryQueue, StateStore,
	tests::{self, FakeTracker, retry_selection},
};

#[test]
fn due_retry_dispatches_when_another_issue_has_active_lease() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = retry_selection::selection_sample_service_owned_issue("In Progress");
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
