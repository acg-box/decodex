use serde::{Deserialize, Serialize};

/// Tracker-facing repository policy.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowTracker {
	provider: TrackerProvider,
	startable_states: Vec<String>,
	terminal_states: Vec<String>,
	in_progress_state: String,
	success_state: String,
	completed_state: String,
	failure_state: String,
	opt_out_label: String,
	needs_attention_label: String,
}
impl WorkflowTracker {
	/// Tracker provider for this repository.
	pub fn provider(&self) -> TrackerProvider {
		self.provider
	}

	/// States that are eligible for automatic execution.
	pub fn startable_states(&self) -> &[String] {
		&self.startable_states
	}

	/// States that are considered terminal for automatic execution.
	pub fn terminal_states(&self) -> &[String] {
		&self.terminal_states
	}

	/// State used when `decodex` starts work on an issue.
	pub fn in_progress_state(&self) -> &str {
		&self.in_progress_state
	}

	/// State used after a successful run and validation pass.
	pub fn success_state(&self) -> &str {
		&self.success_state
	}

	/// Explicit state used after a successful post-merge closeout.
	pub fn completed_state(&self) -> &str {
		&self.completed_state
	}

	/// State used after a successful post-merge closeout.
	pub fn resolved_completed_state(&self) -> &str {
		&self.completed_state
	}

	/// State used when retries are exhausted.
	pub fn failure_state(&self) -> &str {
		&self.failure_state
	}

	/// Label that disables automation for an issue.
	pub fn opt_out_label(&self) -> &str {
		&self.opt_out_label
	}

	/// Label that marks failed runs needing human attention.
	pub fn needs_attention_label(&self) -> &str {
		&self.needs_attention_label
	}
}

/// Repo-local agent defaults.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowAgent {
	transport: String,
}
impl WorkflowAgent {
	/// App-server transport.
	pub fn transport(&self) -> &str {
		&self.transport
	}
}

/// Supported tracker providers.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackerProvider {
	/// Linear issue tracking.
	Linear,
}
