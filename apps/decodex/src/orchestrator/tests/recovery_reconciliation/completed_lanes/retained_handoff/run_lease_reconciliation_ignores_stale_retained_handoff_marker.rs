use time::OffsetDateTime;

use crate::{
	agent::RUN_LEASE_IDLE_TIMEOUT,
	orchestrator::{
		self, ReviewHandoffMarker, RunLeaseDisposition,
		tests::{self, FakeTracker},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn run_lease_reconciliation_ignores_stale_retained_handoff_marker() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue_with_sort_fields(
		"issue-stale-handoff",
		"PUB-205B",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-current";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained review worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);

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

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&ReviewHandoffMarker::new(
			"run-previous",
			1,
			&worktree.branch_name,
			"https://github.com/hack-ink/decodex/pull/205",
			"main",
			&worktree.branch_name,
			&head_oid,
		),
	);

	let now =
		OffsetDateTime::now_utc().unix_timestamp() + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1;
	let actions = orchestrator::inspect_run_lease_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		now,
	)
	.expect("run lease inspection should succeed");

	assert_eq!(actions.len(), 1);
	assert!(matches!(
		actions[0].disposition,
		RunLeaseDisposition::Stalled { idle_for }
			if idle_for >= RUN_LEASE_IDLE_TIMEOUT
	));
}
