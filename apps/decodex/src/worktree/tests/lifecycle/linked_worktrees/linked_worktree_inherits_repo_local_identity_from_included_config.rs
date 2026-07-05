use std::fs;

use crate::worktree::{WorktreeManager, tests};

#[test]
fn linked_worktree_inherits_repo_local_identity_from_included_config() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let included_config = repo_root.parent().unwrap().join("identity.inc");

	tests::run_git(&repo_root, &["config", "--unset-all", "user.name"]);
	tests::run_git(&repo_root, &["config", "--unset-all", "user.email"]);
	fs::write(
			&included_config,
			"[user]\n\tname = Included Tests\n\temail = included@example.com\n[codex]\n\tgithub-identity = y\n\tlinear-workspace = hackink\n",
			)
			.expect("included config should write");
	tests::run_git(
		&repo_root,
		&["config", "--local", "include.path", included_config.to_str().unwrap()],
	);

	let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

	assert_eq!(tests::git_stdout(&spec.path, &["config", "--get", "user.name"]), "Included Tests");
	assert_eq!(
		tests::git_stdout(&spec.path, &["config", "--get", "user.email"]),
		"included@example.com"
	);
	assert_eq!(tests::git_stdout(&spec.path, &["config", "--get", "codex.github-identity"]), "y");
	assert_eq!(
		tests::git_stdout(&spec.path, &["config", "--get", "codex.linear-workspace"]),
		"hackink"
	);
}
