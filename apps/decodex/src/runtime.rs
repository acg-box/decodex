//! Local Decodex control-plane runtime paths and project registry helpers.

use std::{
	cmp::Reverse,
	env, fs,
	path::{Path, PathBuf},
};

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

	state_store.upsert_project(&registration)?;

	Ok(registration)
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
			"Current directory `{}` matches multiple registered Decodex projects; pass `--config <PROJECT_DIR>`.",
			cwd.display()
		);
	}

	Ok(Some(best_project.config_path().to_path_buf()))
}

fn decodex_home_dir_from(home: PathBuf) -> PathBuf {
	home.join(".codex").join("decodex")
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
