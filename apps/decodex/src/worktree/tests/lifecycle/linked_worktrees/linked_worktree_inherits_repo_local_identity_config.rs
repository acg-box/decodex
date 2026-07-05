use crate::worktree::{WorktreeManager, tests};

#[test]
fn linked_worktree_inherits_repo_local_identity_config() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);

	tests::run_git(&repo_root, &["config", "user.signingkey", "worktree-tests"]);
	tests::run_git(&repo_root, &["config", "codex.github-identity", "y"]);
	tests::run_git(&repo_root, &["config", "codex.linear-workspace", "hackink"]);

	let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

	assert_eq!(tests::git_stdout(&spec.path, &["config", "--get", "user.name"]), "Decodex Tests");
	assert_eq!(
		tests::git_stdout(&spec.path, &["config", "--get", "user.email"]),
		"decodex-tests@example.com"
	);
	assert_eq!(tests::git_stdout(&spec.path, &["config", "--get", "commit.gpgsign"]), "false");
	assert_eq!(
		tests::git_stdout(&spec.path, &["config", "--get", "user.signingkey"]),
		"worktree-tests"
	);
	assert_eq!(tests::git_stdout(&spec.path, &["config", "--get", "codex.github-identity"]), "y");
	assert_eq!(
		tests::git_stdout(&spec.path, &["config", "--get", "codex.linear-workspace"]),
		"hackink"
	);
}
