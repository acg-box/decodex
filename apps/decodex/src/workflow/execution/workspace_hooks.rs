use serde::{Deserialize, Serialize};

use crate::{
	prelude::{Result, eyre},
	workflow::validation,
};

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

	pub(in crate::workflow) fn validate(&self) -> Result<()> {
		if self.timeout_seconds == 0 {
			eyre::bail!("`execution.workspace_hooks.timeout_seconds` must be greater than zero.");
		}

		validation::validate_string_entries(
			"execution.workspace_hooks.after_create_commands",
			&self.after_create_commands,
		)?;
		validation::validate_string_entries(
			"execution.workspace_hooks.before_remove_commands",
			&self.before_remove_commands,
		)?;

		Ok(())
	}
}
