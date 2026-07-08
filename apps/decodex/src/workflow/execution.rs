mod gate_profile;
mod repo_gate;
mod workspace_hooks;

pub use self::{
	gate_profile::{WorkflowGateMatchMode, WorkflowGateProfile},
	repo_gate::ResolvedRepoGate,
	workspace_hooks::WorkflowWorkspaceHooks,
};

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
	prelude::{Result, eyre},
	workflow::validation::{self},
};

/// Repo-local execution and repo-gate policy.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowExecution {
	max_attempts: u32,
	max_turns: u32,
	max_retry_backoff_ms: u64,
	canonicalize_commands: Vec<String>,
	verify_commands: Vec<String>,
	gate_profiles: BTreeMap<String, WorkflowGateProfile>,
	workspace_hooks: WorkflowWorkspaceHooks,
}
impl WorkflowExecution {
	/// Maximum automatic attempts before human attention is required.
	pub fn max_attempts(&self) -> u32 {
		self.max_attempts
	}

	/// Maximum same-thread turns per bounded run before Decodex yields cleanly.
	pub fn max_turns(&self) -> u32 {
		self.max_turns
	}

	/// Maximum failure-retry backoff in milliseconds.
	pub fn max_retry_backoff_ms(&self) -> u64 {
		self.max_retry_backoff_ms
	}

	/// Repo canonicalize commands that may rewrite the worktree before verification.
	pub fn canonicalize_commands(&self) -> &[String] {
		&self.canonicalize_commands
	}

	/// Repo verification commands that must pass after canonicalize commands complete.
	pub fn verify_commands(&self) -> &[String] {
		&self.verify_commands
	}

	/// Repo-owned named gate profiles for narrow path-scoped validation.
	pub fn gate_profiles(&self) -> &BTreeMap<String, WorkflowGateProfile> {
		&self.gate_profiles
	}

	/// Repo-owned workspace lifecycle hooks.
	pub fn workspace_hooks(&self) -> &WorkflowWorkspaceHooks {
		&self.workspace_hooks
	}

	/// Full default repo gate declared directly on `[execution]`.
	pub fn default_repo_gate(&self) -> ResolvedRepoGate<'_> {
		ResolvedRepoGate {
			profile_name: None,
			canonicalize_commands: &self.canonicalize_commands,
			verify_commands: &self.verify_commands,
		}
	}

	/// Resolve the repo gate for a concrete changed-file set.
	pub fn select_repo_gate_for_changed_files(
		&self,
		changed_files: &BTreeSet<String>,
	) -> ResolvedRepoGate<'_> {
		if changed_files.is_empty() {
			return self.default_repo_gate();
		}

		let mut matching_profiles = self
			.gate_profiles
			.iter()
			.filter_map(|(profile_name, profile)| {
				profile.matches_changed_files(changed_files).ok().and_then(|matches| {
					matches.then_some(ResolvedRepoGate {
						profile_name: Some(profile_name.as_str()),
						canonicalize_commands: profile.canonicalize_commands(),
						verify_commands: profile.verify_commands(),
					})
				})
			})
			.collect::<Vec<_>>();

		if matching_profiles.len() == 1 {
			return matching_profiles.remove(0);
		}

		self.default_repo_gate()
	}

	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_string_entries(
			"execution.canonicalize_commands",
			&self.canonicalize_commands,
		)?;
		validation::validate_string_entries("execution.verify_commands", &self.verify_commands)?;

		for (profile_name, profile) in &self.gate_profiles {
			let trimmed = profile_name.trim();

			if trimmed.is_empty() {
				eyre::bail!("`execution.gate_profiles` keys must not be empty.");
			}
			if trimmed != profile_name {
				eyre::bail!(
					"`execution.gate_profiles.{profile_name}` must not include surrounding whitespace."
				);
			}

			profile.validate(profile_name)?;
		}

		self.workspace_hooks.validate()?;

		Ok(())
	}
}
