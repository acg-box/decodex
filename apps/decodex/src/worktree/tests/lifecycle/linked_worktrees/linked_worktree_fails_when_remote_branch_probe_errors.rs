use crate::worktree::{WorktreeManager, tests};

#[test]
fn linked_worktree_fails_when_remote_branch_probe_errors() {
	let (temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let missing_remote = temp_dir.path().join("missing-origin.git");

	tests::run_git(&repo_root, &["remote", "set-url", "origin", missing_remote.to_str().unwrap()]);

	let error = manager
		.ensure_worktree("PUB-101", false)
		.expect_err("worktree create should fail when remote probe errors");

	assert!(error.to_string().contains("Failed to inspect remote worktree branch"));
}
