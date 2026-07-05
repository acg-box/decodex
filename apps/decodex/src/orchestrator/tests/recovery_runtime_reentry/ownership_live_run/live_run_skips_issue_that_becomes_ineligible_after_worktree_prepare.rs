use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker},
	},
	state::StateStore,
};

#[test]
fn live_run_skips_issue_that_becomes_ineligible_after_worktree_prepare() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let listed_issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![listed_issue.clone()],
		vec![vec![], vec![listed_issue.clone()], vec![tests::sample_issue("In Progress", &[])]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("run once should succeed");

	assert!(summary.is_none());
	assert!(
		state_store.lease_for_issue(&listed_issue.id).expect("lease lookup should work").is_none()
	);
	assert!(
		state_store
			.worktree_for_issue(&listed_issue.id)
			.expect("worktree lookup should work")
			.is_some()
	);
	assert!(tracker.comments.borrow().is_empty());
}
