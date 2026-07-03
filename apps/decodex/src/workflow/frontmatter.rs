use serde::{Deserialize, Serialize};

use crate::{
	prelude::{Result, eyre},
	workflow::{
		WorkflowAgent, WorkflowContext, WorkflowExecution, WorkflowTracker,
		validation::{self},
	},
};

/// Typed TOML frontmatter for a downstream workflow document.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowFrontmatter {
	version: u8,
	tracker: WorkflowTracker,
	agent: WorkflowAgent,
	execution: WorkflowExecution,
	context: WorkflowContext,
}
impl WorkflowFrontmatter {
	/// Contract version.
	pub fn version(&self) -> u8 {
		self.version
	}

	/// Tracker policy for this repository.
	pub fn tracker(&self) -> &WorkflowTracker {
		&self.tracker
	}

	/// Agent defaults for this repository.
	pub fn agent(&self) -> &WorkflowAgent {
		&self.agent
	}

	/// Execution policy for this repository.
	pub fn execution(&self) -> &WorkflowExecution {
		&self.execution
	}

	/// Extra early-load context paths for this repository.
	pub fn context(&self) -> &WorkflowContext {
		&self.context
	}

	pub(super) fn validate(&self) -> Result<()> {
		if self.version != 1 {
			eyre::bail!("Unsupported WORKFLOW.md version: {}", self.version);
		}

		validation::validate_non_empty_string_list(
			"tracker.startable_states",
			self.tracker.startable_states(),
		)?;
		validation::validate_non_empty_string_list(
			"tracker.terminal_states",
			self.tracker.terminal_states(),
		)?;
		validation::validate_trimmed_non_empty(
			"tracker.in_progress_state",
			self.tracker.in_progress_state(),
		)?;
		validation::validate_trimmed_non_empty(
			"tracker.success_state",
			self.tracker.success_state(),
		)?;
		validation::validate_trimmed_non_empty(
			"tracker.failure_state",
			self.tracker.failure_state(),
		)?;
		validation::validate_trimmed_non_empty(
			"tracker.opt_out_label",
			self.tracker.opt_out_label(),
		)?;
		validation::validate_trimmed_non_empty(
			"tracker.needs_attention_label",
			self.tracker.needs_attention_label(),
		)?;
		validation::validate_trimmed_non_empty("agent.transport", self.agent.transport())?;

		if self.execution.max_attempts() == 0 {
			eyre::bail!("`execution.max_attempts` must be greater than zero.");
		}
		if self.execution.max_turns() == 0 {
			eyre::bail!("`execution.max_turns` must be greater than zero.");
		}
		if self.execution.max_retry_backoff_ms() == 0 {
			eyre::bail!("`execution.max_retry_backoff_ms` must be greater than zero.");
		}

		validation::validate_trimmed_non_empty(
			"tracker.completed_state",
			self.tracker.completed_state(),
		)?;

		if !self
			.tracker
			.terminal_states()
			.iter()
			.any(|state| state == self.tracker.completed_state())
		{
			eyre::bail!("`tracker.completed_state` must be one of `tracker.terminal_states`.");
		}

		self.execution.validate()?;
		self.context.validate()?;

		Ok(())
	}
}
