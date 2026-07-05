use crate::{
	orchestrator::{
		self, IssueDispatchMode,
		tests::{self, FakeTracker, intake_run_and_prompting},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn dry_run_falls_back_to_normal_issue_when_retained_retry_loses_ownership() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let normal_issue = tests::sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:17:17.133Z",
	);
	let retry_issue =
		intake_run_and_prompting::run_and_prompting_service_owned_issue("In Progress");
	let retry_issue_without_ownership = tests::sample_issue("In Progress", &[]);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![normal_issue.clone(), retry_issue.clone()],
		vec![
			vec![retry_issue.clone()],
			vec![retry_issue.clone()],
			vec![retry_issue_without_ownership.clone()],
			vec![retry_issue_without_ownership],
			vec![tests::sample_issue("In Progress", &[])],
			vec![normal_issue.clone()],
		],
	);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&retry_issue.identifier, false)
		.expect("retained retry worktree should exist");

	state_store
		.upsert_worktree(
			config.service_id(),
			&retry_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("dry run should succeed")
		.expect("normal queued issue should be selected after retained retry is excluded");

	assert_eq!(summary.issue_identifier, normal_issue.identifier);
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Normal);
}
