use std::path::{Path, PathBuf};

use crate::{config::ServiceConfig, state};

/// Registered repo target managed by the local Decodex control plane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProjectRegistration {
	pub(in crate::state) service_id: String,
	pub(in crate::state) config_path: PathBuf,
	pub(in crate::state) repo_root: PathBuf,
	pub(in crate::state) worktree_root: PathBuf,
	pub(in crate::state) workflow_path: PathBuf,
	pub(in crate::state) tracker_api_key_env_var: String,
	pub(in crate::state) github_token_env_var: String,
	pub(in crate::state) enabled: bool,
	pub(in crate::state) config_fingerprint: String,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl ProjectRegistration {
	/// Build a registry row from a Decodex project config.
	pub(crate) fn from_config(
		service_id: &str,
		config_path: &Path,
		config: &ServiceConfig,
		enabled: bool,
		config_fingerprint: &str,
	) -> Self {
		let now = state::timestamp_parts();

		Self {
			service_id: service_id.to_owned(),
			config_path: config_path.to_path_buf(),
			repo_root: config.repo_root().to_path_buf(),
			worktree_root: config.worktree_root().to_path_buf(),
			workflow_path: config.workflow_path().to_path_buf(),
			tracker_api_key_env_var: config.tracker().api_key_env_var().to_owned(),
			github_token_env_var: config.github().token_env_var().to_owned(),
			enabled,
			config_fingerprint: config_fingerprint.to_owned(),
			updated_at: now.text,
			updated_at_unix: now.unix,
		}
	}

	/// Stable service id from the project config.
	pub(crate) fn service_id(&self) -> &str {
		&self.service_id
	}

	/// Absolute config path registered for this project.
	pub(crate) fn config_path(&self) -> &Path {
		&self.config_path
	}

	/// Absolute repository root for this project.
	pub(crate) fn repo_root(&self) -> &Path {
		&self.repo_root
	}

	/// Absolute worktree root for this project.
	pub(crate) fn worktree_root(&self) -> &Path {
		&self.worktree_root
	}

	/// Absolute workflow path registered for this project.
	pub(crate) fn workflow_path(&self) -> &Path {
		&self.workflow_path
	}

	/// Environment variable name for the tracker API key.
	pub(crate) fn tracker_api_key_env_var(&self) -> &str {
		&self.tracker_api_key_env_var
	}

	/// Environment variable name for the GitHub token.
	pub(crate) fn github_token_env_var(&self) -> &str {
		&self.github_token_env_var
	}

	/// Whether the project participates in `decodex serve`.
	pub(crate) fn enabled(&self) -> bool {
		self.enabled
	}

	/// Last config fingerprint registered for this project.
	pub(crate) fn config_fingerprint(&self) -> &str {
		&self.config_fingerprint
	}

	/// Last registry update timestamp.
	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	/// Last registry update timestamp as Unix epoch seconds.
	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}

	/// Set whether the registered project is enabled.
	pub(in crate::state) fn set_enabled(&mut self, enabled: bool) {
		self.enabled = enabled;

		let now = state::timestamp_parts();

		self.updated_at = now.text;
		self.updated_at_unix = now.unix;
	}
}
