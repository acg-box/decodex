use std::time::Instant;

use crate::orchestrator::{
	self, IssueDispatchMode, RetryDispatchDecision, RetryEntry, RetryEntryLifecycle, RetryKind,
	RetryQueue, StateStore,
	tests::{self, FakeTracker, retry_selection},
};

#[test]
fn due_retry_claim_stays_queued_when_issue_is_claimed_by_another_process() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = retry_selection::selection_sample_service_owned_issue("In Progress");
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
