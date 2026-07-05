use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker, TEST_SERVICE_ID},
	},
	state::StateStore,
	tracker,
	worktree::WorktreeManager,
};

#[test]
fn run_project_once_recovers_retained_worktree_from_issue_identifier() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue_with_project_slug_and_sort_fields(
		"issue-1",
		"PUB-101",
		"tracker-project",
		"In Progress",
		&[active_label.as_str()],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let expected_worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("recovered worktree should be created")
		.path;
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("recovered dry run should succeed")
		.expect("active recovered issue should be selected");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert_eq!(summary.worktree_path, expected_worktree);
}
