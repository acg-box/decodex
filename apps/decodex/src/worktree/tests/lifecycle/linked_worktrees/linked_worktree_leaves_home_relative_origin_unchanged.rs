use crate::worktree::{self, git, tests};

#[test]
fn linked_worktree_leaves_home_relative_origin_unchanged() {
	let (_temp_dir, repo_root) = tests::init_repo();

	tests::run_git(&repo_root, &["remote", "set-url", "origin", "~/lane-remote.git"]);
	git::normalize_origin_remote_for_worktrees(&repo_root)
		.expect("home-relative remotes should bypass normalization");

	assert_eq!(
		tests::git_stdout(&repo_root, &["remote", "get-url", "origin"]),
		"~/lane-remote.git"
	);
	assert!(!worktree::is_relative_filesystem_remote("~/lane-remote.git"));
	assert!(!worktree::is_relative_filesystem_remote("~"));
}
