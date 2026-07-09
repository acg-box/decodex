use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{
	config::{path_resolution, validation},
	prelude::Result,
};

/// Stable GitHub commit-status context for Decodex fast landing.
pub const FAST_LANDING_STATUS_CONTEXT: &str = "decodex/local-full-check";

/// GitHub landing policy for a target project.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectGitHubLandingMode {
	/// Wait for GitHub's full status rollup and ordinary merge gates.
	#[default]
	Standard,
	/// Trust the Decodex local full-check status and allow configured actors to bypass
	/// GitHub ruleset gates such as code scanning.
	Fast,
}
impl ProjectGitHubLandingMode {
	/// Whether this mode uses the Decodex local validation status as the landing gate.
	pub const fn is_fast(self) -> bool {
		matches!(self, Self::Fast)
	}
}

/// GitHub settings for a target project.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectGitHubConfig {
	token_env_var: String,
	command_path: Option<PathBuf>,
	#[serde(default)]
	landing_mode: ProjectGitHubLandingMode,
	#[serde(default)]
	landing_actors: Vec<String>,
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

	/// Landing policy mode.
	pub const fn landing_mode(&self) -> ProjectGitHubLandingMode {
		self.landing_mode
	}

	/// GitHub users or Apps trusted to publish and execute fast landing.
	pub fn landing_actors(&self) -> &[String] {
		&self.landing_actors
	}

	/// Commit status contexts that can satisfy Decodex landing checks.
	pub fn landing_status_contexts(&self) -> Vec<String> {
		if self.landing_mode.is_fast() {
			vec![String::from(FAST_LANDING_STATUS_CONTEXT)]
		} else {
			Vec::new()
		}
	}

	/// Resolve the configured GitHub token env-var name into a concrete token string.
	pub fn resolve_token(&self) -> Result<String> {
		validation::resolve_secret_env_var("github.token_env_var", self.token_env_var())
	}

	pub(super) fn resolve_paths(mut self, config_dir: &Path) -> Result<Self> {
		if let Some(command_path) = self.command_path.take() {
			validation::validate_nonempty_path("github.command_path", &command_path)?;

			self.command_path =
				Some(path_resolution::resolve_relative_path(config_dir, &command_path));
		}

		Ok(self)
	}

	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_env_var_name("github.token_env_var", self.token_env_var())?;

		if let Some(command_path) = self.command_path.as_deref() {
			validation::validate_nonempty_path("github.command_path", command_path)?;
		}

		if self.landing_mode.is_fast() && self.landing_actors.is_empty() {
			color_eyre::eyre::bail!(
				"`github.landing_actors` must include at least one trusted GitHub actor when `github.landing_mode = \"fast\"`."
			);
		}
		if !self.landing_mode.is_fast() && !self.landing_actors.is_empty() {
			color_eyre::eyre::bail!(
				"`github.landing_actors` is only valid when `github.landing_mode = \"fast\"`."
			);
		}

		for actor in &self.landing_actors {
			validation::validate_required_config_string("github.landing_actors", actor)?;
		}

		Ok(())
	}
}
