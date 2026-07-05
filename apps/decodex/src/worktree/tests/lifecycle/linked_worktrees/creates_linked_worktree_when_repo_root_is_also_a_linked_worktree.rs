use std::{fs, path::PathBuf};

use crate::worktree::{WorktreeManager, tests};

#[test]
fn creates_linked_worktree_when_repo_root_is_also_a_linked_worktree() {
	let (_temp_dir, primary_repo_root) = tests::init_repo();
	let linked_repo_root = primary_repo_root.parent().unwrap().join("linked-root");

	tests::run_git(
		&primary_repo_root,
		&["worktree", "add", "--quiet", "--detach", linked_repo_root.to_str().unwrap(), "HEAD"],
	);
	tests::run_git(
		&linked_repo_root,
		&["checkout", "--quiet", "-B", "x/pubfi-linked-root", "HEAD"],
	);

	let worktree_root = linked_repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &linked_repo_root, &worktree_root);
	let spec = manager
		.ensure_worktree("PUB-101", false)
		.expect("worktree should be created from linked repo root");

	assert_eq!(spec.branch_name, "x/pubfi-pub-101");
	assert!(spec.path.join(".git").is_file());

	let repo_git_dir = fs::canonicalize(PathBuf::from(tests::git_stdout(
		&linked_repo_root,
		&["rev-parse", "--path-format=absolute", "--git-common-dir"],
	)))
	.expect("linked repo common dir should canonicalize");
	let git_dir = fs::canonicalize(PathBuf::from(tests::git_stdout(
		&spec.path,
		&["rev-parse", "--path-format=absolute", "--git-dir"],
	)))
	.expect("git dir should canonicalize");
	let git_common_dir = fs::canonicalize(PathBuf::from(tests::git_stdout(
		&spec.path,
		&["rev-parse", "--path-format=absolute", "--git-common-dir"],
	)))
	.expect("git common dir should canonicalize");

	assert!(git_dir.starts_with(repo_git_dir.join("worktrees")));
	assert_eq!(git_common_dir, repo_git_dir);
}
