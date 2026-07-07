use std::time::Instant;

use crate::{
	orchestrator::{
		self, IssueDispatchMode, RetryDispatchDecision, RetryEntry, RetryEntryLifecycle, RetryKind,
		RetryQueue, StateStore,
		tests::{self, FakeTracker, retry_selection},
	},
	worktree::WorktreeManager,
};

#[test]
fn due_closeout_retry_stays_queued_when_pr_state_read_fails() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(
		&base_config,
		"DECODEX_TEST_MISSING_DELIVERY_CLOSEOUT_GITHUB_TOKEN",
	);
	let issue = retry_selection::selection_sample_service_owned_issue("Done");
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

	tests::seed_review_lifecycle_handoff_fixture_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_lifecycle_handoff_fixture(&worktree.branch_name, pr_url, &head_oid),
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
