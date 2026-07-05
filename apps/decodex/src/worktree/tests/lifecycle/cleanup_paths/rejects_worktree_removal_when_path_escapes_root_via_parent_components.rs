use std::fs;

use crate::worktree::{WorktreeManager, tests};

#[test]
fn rejects_worktree_removal_when_path_escapes_root_via_parent_components() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let escaped_target = repo_root.join("outside").join("PUB-101");

	fs::create_dir_all(&worktree_root).expect("worktree root should exist");
	fs::create_dir_all(&escaped_target).expect("escaped target should exist");

	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let escaped_path = worktree_root.join("../outside/PUB-101");
	let error = manager
		.remove_worktree_path(&escaped_path)
		.expect_err("escaped worktree path should be rejected");

	assert!(error.to_string().contains("outside worktree_root"));
	assert!(escaped_target.exists());
}
