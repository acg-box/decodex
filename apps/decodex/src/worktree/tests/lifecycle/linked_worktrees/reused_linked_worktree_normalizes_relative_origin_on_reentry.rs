use std::fs;

use crate::worktree::{WorktreeManager, tests};

#[test]
fn reused_linked_worktree_normalizes_relative_origin_on_reentry() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let bare_remote = repo_root.parent().unwrap().join("lane-remote.git");
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);

	tests::run_git(
		bare_remote.parent().unwrap(),
		&["init", "--bare", bare_remote.to_str().unwrap()],
	);
	tests::run_git(&repo_root, &["remote", "set-url", "origin", "../lane-remote.git"]);
	tests::run_git(&repo_root, &["push", "-u", "origin", "main"]);

	let created = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

	tests::run_git(&repo_root, &["remote", "set-url", "origin", "../lane-remote.git"]);

	let reused = manager.ensure_worktree("PUB-101", false).expect("worktree should be reused");

	assert!(reused.reused_existing);
	assert_eq!(reused.path, created.path);
	assert_eq!(
		tests::git_stdout(&reused.path, &["remote", "get-url", "origin"]),
		fs::canonicalize(&bare_remote)
			.expect("bare remote should canonicalize")
			.to_str()
			.expect("bare remote should be valid UTF-8")
	);
}
