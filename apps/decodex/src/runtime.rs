//! Local Decodex control-plane runtime paths and project registry helpers.

#[cfg(test)]
use std::process;
use std::{
	cmp::Reverse,
	env, fs,
	io::ErrorKind,
	path::{Path, PathBuf},
};

use toml::Value;

use crate::{
	config::ServiceConfig,
	prelude::{Result, eyre},
	state::{ProjectRegistration, StateStore},
};

/// Resolve Decodex's local application state directory under the Codex home.
pub(crate) fn decodex_home_dir() -> Result<PathBuf> {
	let Some(home) = env::var_os("HOME") else {
		eyre::bail!("Failed to resolve `$HOME` for the local Decodex runtime directory.");
	};

	Ok(decodex_home_dir_from(PathBuf::from(home)))
}

/// Resolve the global operator config path.
pub(crate) fn global_config_path() -> Result<PathBuf> {
	Ok(decodex_home_dir()?.join("config.toml"))
}

/// Read the global fixed account selector, when the operator pinned one.
pub(crate) fn global_fixed_account_selector() -> Result<Option<String>> {
	let config_path = global_config_path()?;
	let input = match fs::read_to_string(&config_path) {
		Ok(input) => input,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
		Err(error) => {
			eyre::bail!(
				"Failed to read Decodex global config `{}`: {error}",
				config_path.display()
			);
		},
	};
	let document = toml::from_str::<toml::Table>(&input)?;
	let selector = document
		.get("codex")
		.and_then(Value::as_table)
		.and_then(|codex| codex.get("accounts"))
		.and_then(Value::as_table)
		.and_then(|accounts| accounts.get("fixed_account"))
		.and_then(Value::as_str)
		.map(str::trim)
		.filter(|value| !value.is_empty())
		.map(str::to_owned);

	Ok(selector)
}

/// Write the global fixed account selector. `None` returns the pool to balanced mode.
#[cfg(test)]
pub(crate) fn write_global_fixed_account_selector(selector: Option<&str>) -> Result<()> {
	let config_path = global_config_path()?;
	let input = match fs::read_to_string(&config_path) {
		Ok(input) => input,
		Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
		Err(error) => {
			eyre::bail!(
				"Failed to read Decodex global config `{}`: {error}",
				config_path.display()
			);
		},
	};
	let mut document = if input.trim().is_empty() {
		toml::Table::new()
	} else {
		toml::from_str::<toml::Table>(&input)?
	};

	match selector.map(str::trim).filter(|value| !value.is_empty()) {
		Some(selector) => {
			let accounts =
				ensure_toml_table(ensure_toml_table(&mut document, "codex")?, "accounts")?;

			accounts.insert(String::from("fixed_account"), selector.to_owned().into());
		},
		None => {
			if let Some(codex) = document.get_mut("codex").and_then(Value::as_table_mut)
				&& let Some(accounts) = codex.get_mut("accounts").and_then(Value::as_table_mut)
			{
				accounts.remove("fixed_account");
			}
		},
	}

	let parent = config_path.parent().ok_or_else(|| {
		eyre::eyre!(
			"Decodex global config `{}` must have a parent directory.",
			config_path.display()
		)
	})?;
	let file_name = config_path
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or_else(|| eyre::eyre!("Decodex global config path must end in a valid file name."))?;
	let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
	let output = toml::to_string_pretty(&document)?;

	fs::create_dir_all(parent)?;
	fs::write(&temp_path, output)?;
	fs::rename(temp_path, &config_path)?;

	Ok(())
}

/// Resolve the global ChatGPT account-pool JSONL path.
pub(crate) fn accounts_path() -> Result<PathBuf> {
	Ok(decodex_home_dir()?.join("accounts.jsonl"))
}

/// Resolve the directory that stores project contract directories managed outside repos.
pub(crate) fn project_config_dir() -> Result<PathBuf> {
	Ok(decodex_home_dir()?.join("projects"))
}

/// Resolve Decodex's log directory.
pub(crate) fn log_dir() -> Result<PathBuf> {
	Ok(decodex_home_dir()?.join("logs"))
}

/// Resolve the local agent-readable evidence directory.
pub(crate) fn agent_evidence_dir() -> Result<PathBuf> {
	Ok(decodex_home_dir()?.join("agent-evidence"))
}

/// Resolve the global single-machine runtime database path.
pub(crate) fn runtime_db_path() -> Result<PathBuf> {
	Ok(decodex_home_dir()?.join("runtime.sqlite3"))
}

/// Open the global single-machine runtime database.
pub(crate) fn open_runtime_store() -> Result<StateStore> {
	StateStore::open(runtime_db_path()?)
}

/// Open the global runtime database without preloading all durable rows.
pub(crate) fn open_runtime_store_lazy() -> Result<StateStore> {
	StateStore::open_lazy(runtime_db_path()?)
}

/// Register or refresh one project config in the global runtime DB.
pub(crate) fn register_project_config(
	state_store: &StateStore,
	config_path: &Path,
	enabled: bool,
) -> Result<ProjectRegistration> {
	let config_path = ServiceConfig::resolve_project_config_path(config_path)?;
	let config_path = fs::canonicalize(config_path)?;
	let config = ServiceConfig::from_path(&config_path)?;
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&config_path,
		&config,
		enabled,
		&config_fingerprint(&config_path, config.workflow_path())?,
	);

	state_store.upsert_project(&registration)
}

/// Resolve the registered project config that owns a local working directory.
pub(crate) fn registered_config_path_for_cwd(
	state_store: &StateStore,
	cwd: &Path,
) -> Result<Option<PathBuf>> {
	let cwd = fs::canonicalize(cwd)?;
	let mut matches = Vec::new();

	for project in state_store.list_projects()? {
		let repo_root = fs::canonicalize(project.repo_root())
			.unwrap_or_else(|_| project.repo_root().to_path_buf());
		let worktree_root = fs::canonicalize(project.worktree_root())
			.unwrap_or_else(|_| project.worktree_root().to_path_buf());
		let matched_root = if cwd.starts_with(&worktree_root) {
			Some(worktree_root)
		} else if cwd.starts_with(&repo_root) {
			Some(repo_root)
		} else {
			None
		};

		if let Some(matched_root) = matched_root {
			matches.push((matched_root.components().count(), project));
		}
	}

	matches.sort_by_key(|item| Reverse(item.0));

	let Some((best_score, best_project)) = matches.first() else {
		return Ok(None);
	};
	let ambiguous = matches.iter().skip(1).any(|(score, project)| {
		score == best_score && project.service_id() != best_project.service_id()
	});

	if ambiguous {
		eyre::bail!(
			"Current directory `{}` matches multiple registered Decodex projects; pass the command's `--config <PROJECT_DIR>`.",
			cwd.display()
		);
	}

	Ok(Some(best_project.config_path().to_path_buf()))
}

/// Resolve one registered project config by stable service id.
pub(crate) fn registered_config_path_for_project_id(
	state_store: &StateStore,
	project_id: &str,
) -> Result<PathBuf> {
	let project_id = project_id.trim();

	if project_id.is_empty() {
		eyre::bail!("Decodex project id cannot be empty.");
	}

	let projects = state_store.list_projects()?;

	if let Some(project) = projects.iter().find(|project| project.service_id() == project_id) {
		return Ok(project.config_path().to_path_buf());
	}

	let registered =
		projects.iter().map(ProjectRegistration::service_id).collect::<Vec<_>>().join(", ");

	eyre::bail!(
		"Decodex project `{project_id}` is not registered. Registered projects: {}.",
		if registered.is_empty() { "none" } else { registered.as_str() }
	)
}

fn decodex_home_dir_from(home: PathBuf) -> PathBuf {
	home.join(".codex").join("decodex")
}

#[cfg(test)]
fn ensure_toml_table<'a>(table: &'a mut toml::Table, key: &str) -> Result<&'a mut toml::Table> {
	if !table.contains_key(key) {
		table.insert(String::from(key), toml::Table::new().into());
	}

	table
		.get_mut(key)
		.and_then(Value::as_table_mut)
		.ok_or_else(|| eyre::eyre!("Decodex global config `{key}` must be a TOML table."))
}

fn config_fingerprint(config_path: &Path, workflow_path: &Path) -> Result<String> {
	let config_body = fs::read(config_path)?;
	let workflow_body = fs::read(workflow_path)?;
	let mut hash = 0xcbf29ce484222325_u64;

	for byte in config_path
		.to_string_lossy()
		.bytes()
		.chain(config_body)
		.chain(workflow_path.to_string_lossy().bytes())
		.chain(workflow_body)
	{
		hash ^= u64::from(byte);
		hash = hash.wrapping_mul(0x100000001b3);
	}

	Ok(format!("{hash:016x}"))
}

#[cfg(test)]
mod tests {
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
		let state_store = StateStore::open(temp_dir.path().join("runtime.sqlite3"))
			.expect("state store should open");
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
		let state_store = StateStore::open(temp_dir.path().join("runtime.sqlite3"))
			.expect("state store should open");
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

		assert!(
			!registration.enabled(),
			"runtime refresh should report the preserved disabled state"
		);
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
		let state_store = StateStore::open(temp_dir.path().join("runtime.sqlite3"))
			.expect("state store should open");
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
		let state_store = StateStore::open(temp_dir.path().join("runtime.sqlite3"))
			.expect("state store should open");
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
}
