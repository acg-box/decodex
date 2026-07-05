use std::path::Path;

use serde::Deserialize;

use crate::{
	config::{ReviewLevel, validation},
	prelude::Result,
};

/// Project-level Codex defaults from service configuration.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
pub struct ProjectCodexConfig {
	#[serde(default)]
	review: ReviewLevel,
	accounts: Option<ProjectCodexAccountsConfig>,
}
impl ProjectCodexConfig {
	/// Review level Decodex should apply for agent runs.
	pub fn review_level(&self) -> ReviewLevel {
		self.review
	}

	/// Optional ChatGPT accounts used to seed Codex app-server auth.
	pub fn accounts(&self) -> Option<&ProjectCodexAccountsConfig> {
		self.accounts.as_ref()
	}

	pub(super) fn resolve_paths(mut self, _config_dir: &Path) -> Result<Self> {
		if let Some(accounts) = self.accounts.take() {
			accounts.validate()?;

			self.accounts = Some(accounts);
		}

		Ok(self)
	}

	pub(super) fn validate(&self) -> Result<()> {
		if let Some(accounts) = &self.accounts {
			accounts.validate()?;
		}

		Ok(())
	}
}

/// Optional JSONL ChatGPT accounts for Codex app-server runs.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectCodexAccountsConfig {
	usage_endpoint: Option<String>,
	profile_endpoint: Option<String>,
	reset_credits_endpoint: Option<String>,
	refresh_endpoint: Option<String>,
}
impl ProjectCodexAccountsConfig {
	/// Override for ChatGPT usage probes. Defaults to the Codex `/wham/usage` endpoint.
	pub fn usage_endpoint(&self) -> Option<&str> {
		self.usage_endpoint.as_deref()
	}

	/// Override for ChatGPT profile-stat probes. Defaults to Codex `/wham/profiles/me`.
	pub fn profile_endpoint(&self) -> Option<&str> {
		self.profile_endpoint.as_deref()
	}

	/// Override for ChatGPT reset-credit probes.
	pub fn reset_credits_endpoint(&self) -> Option<&str> {
		self.reset_credits_endpoint.as_deref()
	}

	/// Override for ChatGPT OAuth refresh. Defaults to the Codex auth token endpoint.
	pub fn refresh_endpoint(&self) -> Option<&str> {
		self.refresh_endpoint.as_deref()
	}

	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_optional_nonempty_string(
			"codex.accounts.usage_endpoint",
			self.usage_endpoint.as_deref(),
		)?;
		validation::validate_optional_nonempty_string(
			"codex.accounts.profile_endpoint",
			self.profile_endpoint.as_deref(),
		)?;
		validation::validate_optional_nonempty_string(
			"codex.accounts.reset_credits_endpoint",
			self.reset_credits_endpoint.as_deref(),
		)?;
		validation::validate_optional_nonempty_string(
			"codex.accounts.refresh_endpoint",
			self.refresh_endpoint.as_deref(),
		)?;

		Ok(())
	}
}
