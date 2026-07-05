use std::process;

use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker},
	},
	state::{self, StateStore},
};

#[test]
fn materialize_run_summary_worktree_creates_worktree_before_child_activity_marker() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("dry-run planning should succeed")
		.expect("brand-new lane should be selected");

	assert!(
		!summary.worktree_path.exists(),
		"dry-run planning should not materialize the worktree yet"
	);

	let worktree = orchestrator::materialize_run_summary_worktree(&config, &workflow, &summary)
		.expect("daemon parent should materialize the worktree before child startup");

	assert_eq!(worktree.path, summary.worktree_path);
	assert_eq!(worktree.branch_name, summary.branch_name);
	assert!(
		worktree.path.exists(),
		"materialized worktree should exist before writing child activity markers"
	);

	state::write_run_activity_marker_for_process(
		&worktree.path,
		&summary.run_id,
		summary.attempt_number,
		process::id(),
	)
	.expect("child activity marker should write after worktree materialization");
}
