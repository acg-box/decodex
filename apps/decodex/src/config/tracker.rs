use serde::Deserialize;

use crate::{config::validation, prelude::Result};

/// Tracker-specific settings for a target project.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTrackerConfig {
	api_key_env_var: String,
	team_id: String,
}
impl ProjectTrackerConfig {
	/// Name of the environment variable that stores the tracker API key.
	pub fn api_key_env_var(&self) -> &str {
		&self.api_key_env_var
	}

	/// Immutable Linear team scope accepted by this project.
	pub fn team_id(&self) -> &str {
		&self.team_id
	}

	/// Resolve the configured tracker API key env-var name into a concrete token string.
	pub fn resolve_api_key(&self) -> Result<String> {
		validation::resolve_secret_env_var("tracker.api_key_env_var", self.api_key_env_var())
	}

	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_env_var_name("tracker.api_key_env_var", self.api_key_env_var())?;
		validation::validate_required_config_string("tracker.team_id", self.team_id())?;

		Ok(())
	}
}
