//! Runtime filesystem path resolution.

use std::{env, path::PathBuf};

use crate::prelude::{Result, eyre};

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

pub(crate) fn decodex_home_dir_from(home: PathBuf) -> PathBuf {
	home.join(".codex").join("decodex")
}
