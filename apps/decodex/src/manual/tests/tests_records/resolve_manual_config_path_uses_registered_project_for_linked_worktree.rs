use std::fs;

use tempfile::TempDir;

#[rustfmt::skip]
use crate::manual::{self, tests};
#[rustfmt::skip]
use crate::test_support::{self, TestEnvVarGuard};
use crate::{runtime, worktree::WorktreeManager};

#[test]
fn resolve_manual_config_path_uses_registered_project_for_linked_worktree() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard = TestEnvVarGuard::set(
		"HOME",
		temp_dir.path().to_str().expect("temp dir path should be utf-8"),
	);
	let repo_root = temp_dir.path().join("target-repo");
	let worktree_root = repo_root.join(".worktrees");
	let config_dir = temp_dir.path().join(".codex/decodex/projects/pubfi");
	let config_path = config_dir.join("project.toml");

	fs::create_dir_all(&repo_root).expect("repo root should exist");
	fs::create_dir_all(&worktree_root).expect("worktree root should exist");
	fs::create_dir_all(&config_dir).expect("config dir should exist");

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

	fs::write(
		&config_path,
		format!(
			r#"
			service_id = "pubfi"

			[tracker]
			api_key_env_var = "HOME"
team_id = "team-test"

			[github]
			token_env_var = "PATH"
owner = "test-owner"
repository = "test-repository"

			[paths]
			repo_root = "{}"
			"#,
			repo_root.display()
		),
	)
	.expect("central project config should write");
	fs::write(config_dir.join("WORKFLOW.md"), "test workflow\n")
		.expect("central workflow should write");
	fs::write(repo_root.join("README.md"), "bootstrap\n").expect("readme should write");
	tests::git_success(&repo_root, &["add", "README.md"]);
	tests::git_success(&repo_root, &["commit", "-m", "bootstrap repo"]);

	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let worktree = manager.ensure_worktree("XY-225", false).expect("worktree should create");
	let state_store = runtime::open_runtime_store().expect("state store should open");
	let canonical_config =
		fs::canonicalize(&config_path).expect("central config should canonicalize");

	runtime::register_project_config(&state_store, &config_path, true)
		.expect("central config should register");

	assert_eq!(
		manual::resolve_manual_config_path(None, &worktree.path)
			.expect("registered config path should resolve"),
		canonical_config
	);
}
