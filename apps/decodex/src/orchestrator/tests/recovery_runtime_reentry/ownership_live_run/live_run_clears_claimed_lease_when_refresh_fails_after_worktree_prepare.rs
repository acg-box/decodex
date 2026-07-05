use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker},
	},
	state::StateStore,
};

#[test]
fn live_run_clears_claimed_lease_when_refresh_fails_after_worktree_prepare() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let listed_issue = tests::sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_error(vec![listed_issue.clone()], "transient refresh failure");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let error = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect_err("run once should propagate refresh failure");

	assert!(
		error.to_string().contains("transient refresh failure"),
		"error should surface the refresh failure"
	);
	assert!(
		state_store.lease_for_issue(&listed_issue.id).expect("lease lookup should work").is_none()
	);
}
