use std::{env, fs, path::Path};

use tempfile::TempDir;

use crate::{
	manual::{self, ManualAuthority, ManualLandRequest, tests},
	runtime,
	test_support::TestEnvVarGuard,
};

#[test]
fn land_authority_validates_issue_override_against_lane() {
	let error = manual::resolve_land_authority(
		Some(Path::new("/tmp/project.toml")),
		Some("XY-999"),
		false,
		Path::new("/tmp/.worktrees/XY-225"),
	)
	.expect_err("mismatched explicit authority should be rejected");

	assert!(error.to_string().contains("does not match the current lane issue `XY-225`"));

	let authority = manual::resolve_land_authority(
		Some(Path::new("/tmp/project.toml")),
		Some("XY-225"),
		false,
		Path::new("/tmp/.worktrees/xy-225"),
	)
	.expect("same issue with different casing should be accepted");

	assert_eq!(authority, ManualAuthority::Issue(String::from("xy-225")));
}

#[test]
fn resolve_authority_accepts_manual_authority() {
	let authority = manual::resolve_authority(
		Some(Path::new("/tmp/project.toml")),
		None,
		true,
		Path::new("/tmp/worktree"),
	)
	.expect("manual authority should resolve");

	assert_eq!(authority, ManualAuthority::Manual);
}

#[test]
fn manual_land_manual_authority_without_config_prepares_unregistered_context() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = tests::init_git_checkout(&temp_dir, "repo");
	let fake_gh_dir = tests::install_fake_repo_view_gh(&temp_dir);
	let path_env = env::var("PATH").unwrap_or_default();
	let _env_guard = TestEnvVarGuard::set_many([
		("GH_TOKEN", String::from("ghp_test")),
		("PATH", format!("{}:{path_env}", fake_gh_dir.display())),
	]);
	let request = ManualLandRequest {
		summary: String::from("ship hotfix"),
		authority: None,
		manual_authority: true,
		pr_url: Some(String::from("https://github.com/hack-ink/decodex/pull/64")),
		related: Vec::new(),
		breaking: false,
	};
	let context = manual::prepare_unregistered_manual_land_context(
		repo_root.clone(),
		repo_root.clone(),
		String::from("main"),
		&request,
	)
	.expect("manual land should prepare without project registry");

	assert_eq!(context.authority, ManualAuthority::Manual);
	assert_eq!(context.service_id, "decodex");
	assert_eq!(
		context.canonical_repo_root,
		fs::canonicalize(&repo_root).expect("repo root should canonicalize")
	);
	assert_eq!(context.project_worktree_root, context.canonical_repo_root.join(".worktrees"));
	assert!(context.workflow.is_none());
	assert!(context.prepared_closeout.is_none());
	assert!(context.review_lifecycle.is_none());
	assert_eq!(context.github_token_env_var, "GH_TOKEN");
	assert_eq!(context.github_token, "ghp_test");
}

#[test]
fn manual_land_manual_authority_with_config_does_not_refresh_project_registry() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let fake_gh_dir = tests::install_fake_repo_view_gh(&temp_dir);
	let path_env = env::var("PATH").unwrap_or_default();
	let _home_guard = TestEnvVarGuard::set_many([
		("HOME", temp_dir.path().to_str().expect("temp dir path should be utf-8").to_owned()),
		("GH_TOKEN", String::from("ghp_test")),
		("PATH", format!("{}:{path_env}", fake_gh_dir.display())),
	]);
	let repo_root = tests::init_git_checkout(&temp_dir, "repo");
	let config_dir = temp_dir.path().join(".codex/decodex/projects/decodex");
	let config_path = config_dir.join("project.toml");
	let request = ManualLandRequest {
		summary: String::from("ship hotfix"),
		authority: None,
		manual_authority: true,
		pr_url: Some(String::from("https://github.com/hack-ink/decodex/pull/64")),
		related: Vec::new(),
		breaking: false,
	};

	fs::create_dir_all(&config_dir).expect("config dir should exist");
	fs::write(
		&config_path,
		format!(
			r#"
			service_id = "decodex"

			[tracker]
			api_key_env_var = "GH_TOKEN"

			[github]
			token_env_var = "GH_TOKEN"

			[paths]
			repo_root = "{}"
			"#,
			repo_root.display()
		),
	)
	.expect("project config should write");
	fs::write(
		config_dir.join("WORKFLOW.md"),
		tests::sample_workflow().to_markdown().expect("sample workflow should render"),
	)
	.expect("workflow should write");

	let context = manual::prepare_configured_manual_land_context(
		repo_root,
		temp_dir.path().join(".worktrees/XY-225"),
		String::from("xy-225"),
		&config_path,
		&request,
	)
	.expect("manual land should prepare with config");
	let state_store = runtime::open_runtime_store().expect("runtime store should open");

	assert!(context.workflow.is_some());
	assert!(context.prepared_closeout.is_none());
	assert!(
		state_store.list_projects().expect("project registry should list").is_empty(),
		"manual-authority land should not refresh project registry unless issue closeout needs runtime state"
	);
}

#[test]
fn resolve_pr_url_requires_explicit_pr_for_manual_authority() {
	let error = manual::resolve_pr_url(None, None, true)
		.expect_err("manual authority land should require explicit pr");

	assert!(error.to_string().contains("--manual-authority"));
}
