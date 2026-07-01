use std::{
	fs,
	path::{Path, PathBuf},
};

use tempfile::TempDir;

use crate::{runtime, state::StateStore, test_support::TestEnvVarGuard};

#[test]
fn runtime_paths_live_under_codex_decodex_home() {
	let home = PathBuf::from("/tmp/decodex-home-test");

	assert_eq!(
		runtime::decodex_home_dir_from(home),
		PathBuf::from("/tmp/decodex-home-test/.codex/decodex")
	);
}

#[test]
fn account_pool_path_lives_under_decodex_home() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let _home_guard = set_test_home(temp_dir.path());

	assert_eq!(
		runtime::accounts_path().expect("accounts path should resolve"),
		temp_dir.path().join(".codex/decodex/accounts.jsonl")
	);
}

#[test]
fn global_fixed_account_selector_round_trips_global_config() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let _home_guard = set_test_home(temp_dir.path());

	assert_eq!(
		runtime::global_fixed_account_selector().expect("missing selector should read"),
		None
	);

	runtime::write_global_fixed_account_selector(Some("copy@example.com"))
		.expect("selector should write");

	assert_eq!(
		runtime::global_fixed_account_selector().expect("selector should read"),
		Some(String::from("copy@example.com"))
	);

	let global_config = fs::read_to_string(
		runtime::global_config_path().expect("global config path should resolve"),
	)
	.expect("global config should exist");

	assert!(global_config.contains("[codex.accounts]"));
	assert!(global_config.contains("fixed_account = \"copy@example.com\""));

	runtime::write_global_fixed_account_selector(None).expect("selector should clear");

	assert_eq!(
		runtime::global_fixed_account_selector().expect("cleared selector should read"),
		None
	);

	let global_config = fs::read_to_string(
		runtime::global_config_path().expect("global config path should resolve"),
	)
	.expect("global config should still exist");

	assert!(!global_config.contains("fixed_account"));
}

#[test]
fn agent_evidence_path_lives_under_decodex_home() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard = set_test_home(temp_dir.path());

	assert_eq!(
		runtime::agent_evidence_dir().expect("agent evidence path should resolve"),
		temp_dir.path().join(".codex/decodex/agent-evidence")
	);
}

#[test]
fn project_config_registration_requires_explicit_repo_root() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard = set_test_home(temp_dir.path());
	let state_store =
		StateStore::open(temp_dir.path().join("runtime.sqlite3")).expect("state store should open");
	let config_dir =
		runtime::project_config_dir().expect("project config dir should resolve").join("pubfi");
	let config_path = config_dir.join("project.toml");

	fs::create_dir_all(&config_dir).expect("project config dir should exist");

	write_workflow(&config_dir);
	write_config_without_repo_root(&config_path);

	let error = runtime::register_project_config(&state_store, &config_dir, true)
		.expect_err("centralized project config without repo_root should fail");

	assert!(
		error.to_string().contains("paths.repo_root"),
		"error should explain the missing explicit repo root: {error:?}"
	);
}

#[test]
fn project_config_refresh_preserves_disabled_state() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard = set_test_home(temp_dir.path());
	let state_store =
		StateStore::open(temp_dir.path().join("runtime.sqlite3")).expect("state store should open");
	let repo_root = temp_dir.path().join("target-repo");
	let config_dir =
		runtime::project_config_dir().expect("project config dir should resolve").join("pubfi");
	let config_path = config_dir.join("project.toml");

	fs::create_dir_all(&repo_root).expect("repo root should exist");
	fs::create_dir_all(&config_dir).expect("project config dir should exist");

	write_workflow(&config_dir);
	write_config_body(&config_path, &repo_root);

	runtime::register_project_config(&state_store, &config_dir, true)
		.expect("project config should register");

	state_store.set_project_enabled("pubfi", false).expect("project should disable");

	let registration = runtime::register_project_config(&state_store, &config_dir, true)
		.expect("project config should refresh");
	let projects = state_store.list_projects().expect("projects should list");

	assert!(!registration.enabled(), "runtime refresh should report the preserved disabled state");
	assert_eq!(projects.len(), 1, "refresh should keep one project row");
	assert!(!projects[0].enabled(), "stored project should remain disabled");
}

fn set_test_home(path: &Path) -> TestEnvVarGuard {
	TestEnvVarGuard::set("HOME", path.to_str().expect("test home should be UTF-8"))
}

#[test]
fn registered_config_path_for_cwd_matches_repo_and_worktree_roots() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = temp_dir.path().join("target-repo");
	let worktree_root = repo_root.join(".worktrees");
	let lane_root = worktree_root.join("XY-380");
	let state_store =
		StateStore::open(temp_dir.path().join("runtime.sqlite3")).expect("state store should open");
	let config_dir = temp_dir.path().join("projects/pubfi");
	let config_path = config_dir.join("project.toml");

	fs::create_dir_all(&repo_root).expect("repo root should exist");
	fs::create_dir_all(&lane_root).expect("lane root should exist");
	fs::create_dir_all(&config_dir).expect("project config dir should exist");

	write_workflow(&config_dir);
	write_config_body(&config_path, &repo_root);

	let registration = runtime::register_project_config(&state_store, &config_dir, true)
		.expect("project config should register");
	let canonical_config = fs::canonicalize(&config_path).expect("config should canonicalize");

	assert_eq!(registration.config_path(), canonical_config.as_path());
	assert_eq!(
		runtime::registered_config_path_for_cwd(&state_store, &repo_root)
			.expect("repo cwd lookup should succeed"),
		Some(canonical_config.clone())
	);
	assert_eq!(
		runtime::registered_config_path_for_cwd(&state_store, &lane_root)
			.expect("worktree cwd lookup should succeed"),
		Some(canonical_config)
	);
}

#[test]
fn registered_config_path_for_project_id_uses_service_id() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = temp_dir.path().join("target-repo");
	let state_store =
		StateStore::open(temp_dir.path().join("runtime.sqlite3")).expect("state store should open");
	let config_dir = temp_dir.path().join("projects/pubfi");
	let config_path = config_dir.join("project.toml");

	fs::create_dir_all(&repo_root).expect("repo root should exist");
	fs::create_dir_all(&config_dir).expect("project config dir should exist");

	write_workflow(&config_dir);
	write_config_body(&config_path, &repo_root);

	runtime::register_project_config(&state_store, &config_dir, true)
		.expect("project config should register");

	assert_eq!(
		runtime::registered_config_path_for_project_id(&state_store, "pubfi")
			.expect("project id lookup should succeed"),
		fs::canonicalize(&config_path).expect("config should canonicalize")
	);
	assert!(
		runtime::registered_config_path_for_project_id(&state_store, "missing")
			.expect_err("unknown project id should fail")
			.to_string()
			.contains("Registered projects: pubfi")
	);
}

fn write_config_body(config_path: &Path, repo_root: &Path) {
	fs::write(
		config_path,
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
	.expect("config should write");
}

fn write_workflow(config_dir: &Path) {
	fs::write(
		config_dir.join("WORKFLOW.md"),
		r#"
+++
version = 1
max_turns = 1

[tracker]
queued_state = "Todo"
in_progress_state = "In Progress"
success_state = "Done"
terminal_states = ["Done", "Canceled"]

[tools]
comment = "issue_comment"
transition = "issue_transition"
label = "issue_label"
progress_checkpoint = "issue_progress_checkpoint"
review_checkpoint = "issue_review_checkpoint"
review_handoff = "issue_review_handoff"
terminal_finalize = "issue_terminal_finalize"
+++

Follow the project policy.
"#,
	)
	.expect("workflow should write");
}

fn write_config_without_repo_root(config_path: &Path) {
	fs::write(
		config_path,
		r#"
service_id = "pubfi"

[tracker]
api_key_env_var = "HOME"

[github]
token_env_var = "PATH"
"#,
	)
	.expect("config should write");
}
