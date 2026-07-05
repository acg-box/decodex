use std::fs;

use crate::worktree::{WorktreeManager, tests};

#[test]
fn linked_worktree_push_uses_normalized_absolute_origin_when_source_remote_is_relative() {
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

	let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

	fs::write(spec.path.join("WORKTREE.md"), "linked worktree lane\n")
		.expect("worktree file should write");
	tests::run_git(&spec.path, &["add", "WORKTREE.md"]);
	tests::run_git(&spec.path, &["commit", "-m", "worktree change"]);
	tests::run_git(&spec.path, &["push", "-u", "origin", "x/pubfi-pub-101"]);

	assert_eq!(
		tests::git_stdout(&spec.path, &["remote", "get-url", "origin"]),
		fs::canonicalize(&bare_remote)
			.expect("bare remote should canonicalize")
			.to_str()
			.expect("bare remote should be valid UTF-8")
	);
}
