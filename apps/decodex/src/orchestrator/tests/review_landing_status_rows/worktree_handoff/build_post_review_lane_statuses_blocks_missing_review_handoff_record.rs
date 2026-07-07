use crate::{
	orchestrator::{
		self, StateStore, tests,
		tests::{FakePullRequestReviewStateInspector, FakeTracker},
	},
	worktree::WorktreeManager,
};

#[test]
fn build_post_review_lane_statuses_blocks_missing_review_handoff_record() {
	for managed_worktree in [false, true] {
		let (_temp_dir, config, workflow) = tests::temp_project_layout();
		let issue = tests::sample_issue("In Review", &[]);
		let tracker =
			FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
		let state_store = StateStore::open_in_memory().expect("state store should open");

		if managed_worktree {
			let worktree_manager = WorktreeManager::new(
				config.service_id(),
				config.repo_root(),
				config.worktree_root(),
			);
			let worktree = worktree_manager
				.ensure_worktree(&issue.identifier, false)
				.expect("worktree should exist");

			state_store
				.upsert_worktree(
					config.service_id(),
					&issue.id,
					&worktree.branch_name,
					&worktree.path.display().to_string(),
				)
				.expect("worktree should record");
		} else {
			let repo_root = config.repo_root().to_path_buf();

			state_store
				.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
				.expect("worktree should record");
		}

		let lanes = orchestrator::build_post_review_lane_statuses(
			&tracker,
			&config,
			&workflow,
			&state_store,
			&FakePullRequestReviewStateInspector::new(Vec::new()),
		)
		.expect("post-review lane status build should succeed");

		assert_eq!(lanes.len(), 1);
		assert_eq!(lanes[0].classification, "blocked");
		assert_eq!(lanes[0].reason, "missing_review_lifecycle_record");
	}
}
