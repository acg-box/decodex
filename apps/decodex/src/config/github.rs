use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{
	config::{self, validation},
	prelude::Result,
};

/// GitHub settings for a target project.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectGitHubConfig {
	token_env_var: String,
	command_path: Option<PathBuf>,
}
impl ProjectGitHubConfig {
	/// Name of the environment variable that stores the GitHub token.
	pub fn token_env_var(&self) -> &str {
		&self.token_env_var
	}

	/// Optional configured GitHub CLI command path.
	pub fn command_path(&self) -> Option<&Path> {
		self.command_path.as_deref()
	}

	/// Resolve the configured GitHub token env-var name into a concrete token string.
	pub fn resolve_token(&self) -> Result<String> {
		validation::resolve_secret_env_var("github.token_env_var", self.token_env_var())
	}

	pub(super) fn resolve_paths(mut self, config_dir: &Path) -> Result<Self> {
		if let Some(command_path) = self.command_path.take() {
			validation::validate_nonempty_path("github.command_path", &command_path)?;

			self.command_path = Some(config::resolve_relative_path(config_dir, &command_path));
		}

		Ok(self)
	}

	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_env_var_name("github.token_env_var", self.token_env_var())?;

		if let Some(command_path) = self.command_path.as_deref() {
			validation::validate_nonempty_path("github.command_path", command_path)?;
		}

		Ok(())
	}
}
