use crate::worktree::{WorktreeManager, git, tests};

#[test]
fn linked_worktree_rolls_back_when_origin_normalization_fails() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let spec = manager.plan_for_issue("PUB-101");

	tests::run_git(&repo_root, &["remote", "set-url", "origin", "../missing-remote.git"]);

	let error = manager
		.ensure_worktree("PUB-101", false)
		.expect_err("worktree creation should fail when origin normalization fails");

	assert!(
		error.to_string().contains("No such file or directory")
			|| error.to_string().contains("does not exist"),
		"unexpected error: {error:?}"
	);
	assert!(!spec.path.exists(), "failed setup should remove the new worktree path");
	assert!(
		!git::worktree_is_registered(&repo_root, &spec.path)
			.expect("worktree registration should inspect"),
		"failed setup should unregister the new worktree"
	);
}
