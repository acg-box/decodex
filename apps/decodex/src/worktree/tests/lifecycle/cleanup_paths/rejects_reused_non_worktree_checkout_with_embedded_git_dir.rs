use std::fs;

use crate::worktree::{WorktreeManager, tests};

#[test]
fn rejects_reused_non_worktree_checkout_with_embedded_git_dir() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let worktree_path = worktree_root.join("PUB-101");

	fs::create_dir_all(&worktree_root).expect("worktree root should exist");
	tests::run_git(
		&repo_root,
		&["clone", "--quiet", "--no-checkout", ".", worktree_path.to_str().unwrap()],
	);

	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let error = manager
		.ensure_worktree("PUB-101", false)
		.expect_err("embedded git checkout should be rejected");

	assert!(
		error
			.to_string()
			.contains("is not a linked git worktree: expected `.git` to be a pointer file")
	);
}
