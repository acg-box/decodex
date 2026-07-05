use std::fs;

use crate::worktree::{WorktreeManager, tests};

#[test]
fn linked_worktree_uses_existing_remote_lane_branch_when_present() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let bare_remote = repo_root.parent().unwrap().join("lane-remote.git");
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let lane_branch = "x/pubfi-pub-101";

	tests::run_git(
		bare_remote.parent().unwrap(),
		&["init", "--bare", bare_remote.to_str().unwrap()],
	);
	tests::run_git(&repo_root, &["remote", "set-url", "origin", "../lane-remote.git"]);
	tests::run_git(&repo_root, &["push", "-u", "origin", "main"]);
	tests::run_git(&repo_root, &["checkout", "-b", lane_branch]);
	fs::write(repo_root.join("LANE.md"), "lane branch\n").expect("lane file should write");
	tests::run_git(&repo_root, &["add", "LANE.md"]);
	tests::run_git(&repo_root, &["commit", "-m", "lane branch"]);
	tests::run_git(&repo_root, &["push", "-u", "origin", lane_branch]);
	tests::run_git(&repo_root, &["checkout", "main"]);

	let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

	assert_eq!(tests::git_stdout(&spec.path, &["rev-parse", "--abbrev-ref", "HEAD"]), lane_branch);
	assert_eq!(
		fs::read_to_string(spec.path.join("LANE.md")).expect("lane file should exist"),
		"lane branch\n"
	);
	assert_eq!(
		tests::git_stdout(&spec.path, &["remote", "get-url", "origin"]),
		fs::canonicalize(&bare_remote)
			.expect("bare remote should canonicalize")
			.to_str()
			.expect("bare remote should be valid UTF-8")
	);
}
