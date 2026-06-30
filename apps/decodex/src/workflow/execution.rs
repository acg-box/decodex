use std::{
	collections::{BTreeSet, HashMap},
	path::Path,
};

use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

use crate::prelude::eyre;

use super::validation::{validate_repo_relative_paths, validate_string_entries};

/// Repo-local execution and repo-gate policy.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowExecution {
	max_attempts: u32,
	max_turns: u32,
	max_retry_backoff_ms: u64,
	canonicalize_commands: Vec<String>,
	verify_commands: Vec<String>,
	gate_profiles: HashMap<String, WorkflowGateProfile>,
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
	pub fn gate_profiles(&self) -> &HashMap<String, WorkflowGateProfile> {
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

	pub(super) fn validate(&self) -> crate::prelude::Result<()> {
		validate_string_entries("execution.canonicalize_commands", &self.canonicalize_commands)?;
		validate_string_entries("execution.verify_commands", &self.verify_commands)?;

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

/// Repo-owned workspace lifecycle hooks around linked worktree setup and cleanup.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowWorkspaceHooks {
	after_create_commands: Vec<String>,
	before_remove_commands: Vec<String>,
	timeout_seconds: u64,
}
impl WorkflowWorkspaceHooks {
	/// Commands that run after Decodex creates a new linked worktree for a lane.
	pub fn after_create_commands(&self) -> &[String] {
		&self.after_create_commands
	}

	/// Commands that run before Decodex removes a linked worktree for a lane.
	pub fn before_remove_commands(&self) -> &[String] {
		&self.before_remove_commands
	}

	/// Shared timeout budget, in seconds, for each workspace hook command.
	pub fn timeout_seconds(&self) -> u64 {
		self.timeout_seconds
	}

	fn validate(&self) -> crate::prelude::Result<()> {
		if self.timeout_seconds == 0 {
			eyre::bail!("`execution.workspace_hooks.timeout_seconds` must be greater than zero.");
		}

		validate_string_entries(
			"execution.workspace_hooks.after_create_commands",
			&self.after_create_commands,
		)?;
		validate_string_entries(
			"execution.workspace_hooks.before_remove_commands",
			&self.before_remove_commands,
		)?;

		Ok(())
	}
}

/// Narrow, repo-owned gate profile selected from changed tracked files.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowGateProfile {
	match_mode: WorkflowGateMatchMode,
	paths: Vec<String>,
	canonicalize_commands: Vec<String>,
	verify_commands: Vec<String>,
}
impl WorkflowGateProfile {
	/// Match mode for the profile.
	pub fn match_mode(&self) -> WorkflowGateMatchMode {
		self.match_mode
	}

	/// Repo-relative path patterns covered by this profile.
	pub fn paths(&self) -> &[String] {
		&self.paths
	}

	/// Canonicalize commands for this profile.
	pub fn canonicalize_commands(&self) -> &[String] {
		&self.canonicalize_commands
	}

	/// Verify commands for this profile.
	pub fn verify_commands(&self) -> &[String] {
		&self.verify_commands
	}

	fn validate(&self, profile_name: &str) -> crate::prelude::Result<()> {
		if self.paths.is_empty() {
			eyre::bail!("`execution.gate_profiles.{profile_name}.paths` must not be empty.");
		}
		if self.canonicalize_commands.is_empty() && self.verify_commands.is_empty() {
			eyre::bail!(
				"`execution.gate_profiles.{profile_name}` must declare at least one canonicalize or verify command."
			);
		}

		validate_repo_relative_paths(
			&format!("execution.gate_profiles.{profile_name}.paths"),
			&self.paths,
		)?;

		self.compile_path_set(profile_name)?;

		validate_string_entries(
			&format!("execution.gate_profiles.{profile_name}.canonicalize_commands"),
			&self.canonicalize_commands,
		)?;
		validate_string_entries(
			&format!("execution.gate_profiles.{profile_name}.verify_commands"),
			&self.verify_commands,
		)?;

		Ok(())
	}

	fn matches_changed_files(
		&self,
		changed_files: &BTreeSet<String>,
	) -> crate::prelude::Result<bool> {
		let path_set = self.compile_path_set("runtime")?;

		match self.match_mode {
			WorkflowGateMatchMode::Only =>
				Ok(changed_files.iter().all(|path| path_set.is_match(Path::new(path)))),
		}
	}

	fn compile_path_set(&self, profile_name: &str) -> crate::prelude::Result<GlobSet> {
		let mut builder = GlobSetBuilder::new();

		for path in &self.paths {
			let glob = Glob::new(path).map_err(|error| {
				eyre::eyre!(
					"Invalid glob pattern in `execution.gate_profiles.{profile_name}.paths`: `{path}` ({error})"
				)
			})?;

			builder.add(glob);
		}

		builder.build().map_err(|error| {
			eyre::eyre!("Failed to compile `execution.gate_profiles.{profile_name}.paths`: {error}")
		})
	}
}

/// A resolved repo gate ready to execute.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedRepoGate<'a> {
	profile_name: Option<&'a str>,
	canonicalize_commands: &'a [String],
	verify_commands: &'a [String],
}
impl<'a> ResolvedRepoGate<'a> {
	/// Optional selected profile name; `None` means the default full gate.
	pub fn profile_name(&self) -> Option<&'a str> {
		self.profile_name
	}

	/// Canonicalize commands selected for this gate run.
	pub fn canonicalize_commands(&self) -> &'a [String] {
		self.canonicalize_commands
	}

	/// Verification commands selected for this gate run.
	pub fn verify_commands(&self) -> &'a [String] {
		self.verify_commands
	}
}

/// Match semantics for a repo-owned gate profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowGateMatchMode {
	/// The profile applies only when every changed tracked file is covered by its path set.
	Only,
}
