use std::fs;

use tempfile::TempDir;

#[rustfmt::skip]
use crate::manual::{self, tests};
#[rustfmt::skip]
use crate::test_support::{self, TestEnvVarGuard};
use crate::{
	config::ServiceConfig,
	runtime,
	state::{ReviewHandoffMarker, StateStore},
	worktree::WorktreeManager,
};

#[test]
fn manual_land_closeout_marker_roundtrips() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let checkout = tests::init_git_checkout(&temp_dir, "repo");

	manual::write_manual_land_closeout_marker(
		&checkout,
		"https://github.com/hack-ink/decodex/pull/67",
		"deadbeef",
		"xy-225",
		r#"{"schema":"decodex/commit/1"}"#,
	)
	.expect("closeout marker should write");

	assert!(
		manual::manual_land_closeout_matches(
			&checkout,
			"https://github.com/hack-ink/decodex/pull/67",
			"deadbeef",
			"xy-225",
			r#"{"schema":"decodex/commit/1"}"#,
		)
		.expect("closeout marker should read"),
	);

	let marker = manual::read_manual_land_closeout_marker(&checkout)
		.expect("closeout marker should parse")
		.expect("closeout marker should exist");

	assert_eq!(marker.landed_change.as_deref(), Some(r#"{"schema":"decodex/commit/1"}"#));
	assert!(
		!checkout.join(".decodex/manual-land-closeout").exists(),
		"closeout marker should live under git admin state, not the working tree"
	);
}

#[test]
fn manual_land_closeout_marker_rejects_mismatched_receipts() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let checkout = tests::init_git_checkout(&temp_dir, "repo");

	manual::write_manual_land_closeout_marker(
		&checkout,
		"https://github.com/hack-ink/decodex/pull/67",
		"deadbeef",
		"xy-225",
		r#"{"schema":"decodex/commit/1"}"#,
	)
	.expect("closeout marker should write");

	assert!(
		!manual::manual_land_closeout_matches(
			&checkout,
			"https://github.com/hack-ink/decodex/pull/67",
			"cafebabe",
			"xy-225",
			r#"{"schema":"decodex/commit/1"}"#,
		)
		.expect("closeout marker should compare"),
	);
}

#[test]
fn manual_land_handoff_lookup_prefers_current_branch_record() {
	let issue = tests::sample_issue("issue-1", "XY-225", true, &["decodex:active:pubfi"]);
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_review_handoff_marker(
			"decodex",
			&issue.id,
			&ReviewHandoffMarker::new(
				String::from("run-current"),
				2,
				String::from("xy-225"),
				String::from("https://github.com/hack-ink/decodex/pull/67"),
				String::from("main"),
				String::from("xy-225"),
				String::from("deadbeef"),
			),
		)
		.expect("runtime handoff should persist");
	state_store
		.upsert_review_handoff_marker(
			"decodex",
			&issue.id,
			&ReviewHandoffMarker::new(
				String::from("run-other"),
				3,
				String::from("xy-225-next"),
				String::from("https://github.com/hack-ink/decodex/pull/99"),
				String::from("main"),
				String::from("xy-225-next"),
				String::from("cafebabe"),
			),
		)
		.expect("unrelated runtime handoff should persist");

	let handoff = manual::read_manual_land_handoff(&state_store, "decodex", &issue.id, "xy-225")
		.expect("manual land handoff should read")
		.expect("current branch handoff should be found");

	assert_eq!(handoff.branch_name(), "xy-225");
	assert_eq!(handoff.pr_url(), "https://github.com/hack-ink/decodex/pull/67");
}

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

			[github]
			token_env_var = "PATH"

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

#[test]
fn ensure_cli_repo_context_rejects_foreign_config_repo_root() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let current_repo = tests::init_git_checkout(&temp_dir, "current-repo");
	let foreign_repo = tests::init_git_checkout(&temp_dir, "foreign-repo");
	let config_path = foreign_repo.join("project.toml");

	fs::write(
		&config_path,
		r#"
			service_id = "pubfi"

			[tracker]
			api_key_env_var = "HOME"

			[github]
			token_env_var = "PATH"

			[paths]
			repo_root = "."
			"#,
	)
	.expect("foreign config should write");

	let config = ServiceConfig::from_path(&config_path).expect("config should parse");
	let canonical_repo_root =
		fs::canonicalize(&current_repo).expect("current repo root should canonicalize");
	let error = manual::ensure_cli_repo_context(&current_repo, &config, &canonical_repo_root)
		.expect_err("foreign config repo root should be rejected");

	assert!(error.to_string().contains("does not match loaded config repo root"));
	assert!(error.to_string().contains(&foreign_repo.display().to_string()));
}
