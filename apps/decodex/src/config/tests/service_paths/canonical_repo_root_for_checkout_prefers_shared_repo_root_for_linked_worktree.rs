use std::fs;

use tempfile::TempDir;

use crate::{config, test_support, worktree::WorktreeManager};

#[test]
fn canonical_repo_root_for_checkout_prefers_shared_repo_root_for_linked_worktree() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let repo_root = temp_dir.path().join("target-repo");
	let worktree_root = repo_root.join(".worktrees");

	fs::create_dir_all(&repo_root).expect("repo root should exist");
	fs::create_dir_all(&worktree_root).expect("worktree root should exist");

	assert!(
		test_support::hermetic_git_command()
			.args(["init", "-b", "main"])
			.current_dir(temp_dir.path())
			.arg(&repo_root)
			.status()
			.expect("git init should run")
			.success()
	);
	assert!(
		test_support::hermetic_git_command()
			.args(["config", "user.name", "Decodex Tests"])
			.current_dir(&repo_root)
			.status()
			.expect("git config should run")
			.success()
	);
	assert!(
		test_support::hermetic_git_command()
			.args(["config", "user.email", "decodex-tests@example.com"])
			.current_dir(&repo_root)
			.status()
			.expect("git config should run")
			.success()
	);
	assert!(
		test_support::hermetic_git_command()
			.args(["config", "commit.gpgsign", "false"])
			.current_dir(&repo_root)
			.status()
			.expect("git config should run")
			.success()
	);

	fs::write(repo_root.join("README.md"), "bootstrap\n").expect("readme should write");

	assert!(
		test_support::hermetic_git_command()
			.args(["add", "README.md"])
			.current_dir(&repo_root)
			.status()
			.expect("git add should run")
			.success()
	);
	assert!(
		test_support::hermetic_git_command()
			.args(["commit", "-m", "seed repo"])
			.current_dir(&repo_root)
			.status()
			.expect("git commit should run")
			.success()
	);

	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let worktree = manager.ensure_worktree("XY-251", false).expect("worktree should create");
	let canonical_repo_root = fs::canonicalize(&repo_root).expect("repo root should canonicalize");

	assert_eq!(
		config::canonical_repo_root_for_checkout(&worktree.path)
			.expect("canonical repo root should resolve")
			.expect("linked worktree should expose a canonical repo root"),
		canonical_repo_root
	);
}
