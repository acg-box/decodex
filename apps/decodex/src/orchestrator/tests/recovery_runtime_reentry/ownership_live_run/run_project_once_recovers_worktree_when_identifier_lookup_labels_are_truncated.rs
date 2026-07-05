use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker, TEST_SERVICE_ID, recovery_terminal_support},
	},
	state::StateStore,
	tracker,
	worktree::WorktreeManager,
};

#[test]
fn run_project_once_recovers_worktree_when_identifier_lookup_labels_are_truncated() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let listed_issue = recovery_terminal_support::sample_active_issue("In Progress");
	let mut identifier_lookup_issue = listed_issue.clone();

	identifier_lookup_issue.labels_complete = false;

	identifier_lookup_issue.labels.retain(|label| label.name != active_label);

	let tracker = FakeTracker::with_refresh_snapshots(
		vec![listed_issue.clone()],
		vec![vec![listed_issue.clone()]],
	)
	.with_identifier_lookup_issues(vec![identifier_lookup_issue]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let expected_worktree = worktree_manager
		.ensure_worktree(&listed_issue.identifier, false)
		.expect("recovered worktree should be created")
		.path;
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("recovery should succeed")
		.expect("ambiguous label pagination should still recover the owned retained lane");

	assert_eq!(summary.issue_id, listed_issue.id);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert_eq!(summary.worktree_path, expected_worktree);
}
