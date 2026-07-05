use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn candidate_selection_blocks_ordinary_dispatch_for_retained_review_handoff_marker() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("Todo", &[]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/174";

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![issue],
		&workflow,
		&state_store,
		config.service_id(),
	)
	.expect("candidate selection should succeed");

	assert!(
		selected.is_none(),
		"ordinary intake must not mint a duplicate attempt for a retained review handoff lane"
	);
}
