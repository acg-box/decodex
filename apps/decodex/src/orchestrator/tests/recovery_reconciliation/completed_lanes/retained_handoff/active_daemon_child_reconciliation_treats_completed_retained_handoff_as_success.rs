use time::OffsetDateTime;

use crate::{
	agent::RUN_LEASE_IDLE_TIMEOUT,
	orchestrator::{
		self, ChildRunRef, CurrentChildRunContext, IssueDispatchMode, RunLeaseDisposition,
		tests::{self, FakeTracker},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn active_daemon_child_reconciliation_treats_completed_retained_handoff_as_success() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue_with_sort_fields(
		"issue-daemon-handoff-complete",
		"PUB-206",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-1";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained review worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/206";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");
	state_store
		.append_event(run_id, 1, "thread/status/changed", "{\"status\":\"active\"}")
		.expect("stale run activity should record");

	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_lifecycle_handoff_fixture(&worktree.branch_name, pr_url, &head_oid),
	);

	let now =
		OffsetDateTime::now_utc().unix_timestamp() + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1;
	let actions = orchestrator::inspect_current_daemon_child_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		CurrentChildRunContext {
			child: ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
			workflow: &workflow,
			dispatch_mode: IssueDispatchMode::Normal,
		},
		now,
	)
	.expect("current daemon-child inspection should succeed");

	assert_eq!(actions.len(), 1);
	assert!(matches!(actions[0].disposition, RunLeaseDisposition::RetainedReviewComplete));
}
