use std::fs;

use crate::{
	orchestrator::{
		self, RecoverableWorktreeSkipCache,
		tests::{self, FakeTracker},
	},
	state::StateStore,
};

#[test]
fn recovery_skip_cache_suppresses_repeated_unowned_worktree_lookup() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(Vec::new()).with_identifier_lookup_issues(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let mut skip_cache = RecoverableWorktreeSkipCache::default();

	fs::create_dir_all(&worktree_path).expect("stale worktree directory should exist");

	let first = orchestrator::recover_runtime_state_from_tracker_and_worktrees_with_skip_cache(
		&tracker,
		&config,
		&workflow,
		&state_store,
		Some(&mut skip_cache),
	)
	.expect("first recovery probe should succeed");
	let second = orchestrator::recover_runtime_state_from_tracker_and_worktrees_with_skip_cache(
		&tracker,
		&config,
		&workflow,
		&state_store,
		Some(&mut skip_cache),
	)
	.expect("cached recovery probe should succeed");
	let identifier_queries = tracker.identifier_queries.borrow();

	assert!(first.recoverable_issues.is_empty());
	assert!(second.recoverable_issues.is_empty());
	assert_eq!(identifier_queries.len(), 1);
	assert_eq!(identifier_queries[0], issue.identifier);
	assert!(
		tracker.refresh_queries.borrow().is_empty(),
		"empty known issue sets should not call tracker refresh"
	);
	assert!(
		tracker.label_queries.borrow().is_empty(),
		"complete issue labels should not need server confirmation"
	);
}
