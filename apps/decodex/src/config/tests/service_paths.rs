use std::fs;

use tempfile::TempDir;

use crate::{
	config::{self, ReviewLevel, ServiceConfig, tests},
	test_support,
	worktree::WorktreeManager,
};

#[test]
fn loads_service_config_from_project_file_with_explicit_repo_root() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = tests::write_config_file(
		temp_dir.path(),
		r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"
				command_path = "bin/gh"
			"#,
	);
	let config =
		ServiceConfig::from_path(&config_path).expect("service config should load from disk");
	let canonical_root = fs::canonicalize(temp_dir.path()).expect("temp dir should canonicalize");

	assert_eq!(config.service_id(), "pubfi");
	assert_eq!(config.repo_root(), canonical_root);
	assert_eq!(config.worktree_root(), canonical_root.join(".worktrees"));
	assert_eq!(config.workflow_path(), canonical_root.join("WORKFLOW.md"));
	assert_eq!(config.github().token_env_var(), "HOME");
	assert_eq!(config.github().command_path(), Some(canonical_root.join("bin/gh").as_path()));
	assert_eq!(config.codex().review_level(), ReviewLevel::Strict);
}

#[test]
fn loads_service_config_from_project_directory() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = tests::write_config_file(
		temp_dir.path(),
		r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"
			"#,
	);
	let config = ServiceConfig::from_path(temp_dir.path())
		.expect("service config should load from project directory");

	assert_eq!(config.service_id(), "pubfi");
	assert_eq!(
		ServiceConfig::resolve_project_config_path(temp_dir.path())
			.expect("project directory should resolve"),
		config_path
	);
}

#[test]
fn loads_service_config_with_relative_worktree_override() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = tests::write_config_file(
		temp_dir.path(),
		r#"
				service_id = "pubfi"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"

				[paths]
				worktree_root = "var/worktrees"
			"#,
	);
	let config =
		ServiceConfig::from_path(&config_path).expect("service config should load from disk");
	let canonical_root = fs::canonicalize(temp_dir.path()).expect("temp dir should canonicalize");

	assert_eq!(config.worktree_root(), canonical_root.join("var/worktrees"));
}

#[test]
fn loads_service_config_from_external_project_file_with_explicit_repo_root() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let repo_root = temp_dir.path().join("target-repo");
	let config_dir = temp_dir.path().join("codex/decodex/projects/rsnap");
	let config_path = config_dir.join("project.toml");

	fs::create_dir_all(&repo_root).expect("repo root should exist");
	fs::create_dir_all(&config_dir).expect("config dir should exist");
	fs::write(
		&config_path,
		r#"
				service_id = "rsnap"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"

				[paths]
				repo_root = "../../../../target-repo"
				worktree_root = "lanes"
			"#,
	)
	.expect("centralized config should write");

	let config = ServiceConfig::from_path(&config_path).expect("centralized config should load");
	let canonical_root = fs::canonicalize(&repo_root).expect("repo root should canonicalize");

	assert_eq!(config.service_id(), "rsnap");
	assert_eq!(config.repo_root(), canonical_root);
	assert_eq!(config.worktree_root(), canonical_root.join("lanes"));
	assert_eq!(
		config.workflow_path(),
		fs::canonicalize(&config_dir).expect("config dir should canonicalize").join("WORKFLOW.md")
	);
}

#[test]
fn rejects_project_config_with_nonstandard_file_name() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = temp_dir.path().join("rsnap.toml");

	fs::write(&config_path, "").expect("config should write");

	let error = ServiceConfig::from_path(&config_path)
		.expect_err("nonstandard config file name should fail");

	assert!(
		error.to_string().contains("project.toml"),
		"error should explain the fixed config file name: {error:?}"
	);
}

#[test]
fn external_project_config_requires_explicit_repo_root() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let config_path = temp_dir.path().join("project.toml");

	fs::write(
		&config_path,
		r#"
				service_id = "rsnap"

				[tracker]
				api_key_env_var = "HOME"

				[github]
				token_env_var = "HOME"
			"#,
	)
	.expect("centralized config should write");

	let error = ServiceConfig::from_path(&config_path).expect_err("repo_root should be required");

	assert!(
		error.to_string().contains("paths.repo_root"),
		"error should explain the missing explicit repo root: {error:?}"
	);
}

#[test]
#[cfg(unix)]
fn git_path_output_preserves_non_utf8_bytes() {
	let path = config::path_buf_from_git_line_output(b"/tmp/\xFFlane\n")
		.expect("git path output should parse")
		.expect("git path output should not be empty");

	assert_eq!(std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str()), b"/tmp/\xFFlane");
}

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
