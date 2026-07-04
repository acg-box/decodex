use crate::{
	execution_program::{
		model::ExecutionLinearIssueMapping,
		validation::{self},
	},
	prelude::{Result, eyre},
	tracker,
	workflow::WorkflowDocument,
};

/// Workflow policy needed for Execution Program readiness.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionWorkflowPolicy {
	pub(in crate::execution_program) service_id: String,
	queue_label: String,
	pub(in crate::execution_program) active_label: String,
	startable_states: Vec<String>,
	terminal_states: Vec<String>,
	pub(in crate::execution_program) opt_out_label: String,
	pub(in crate::execution_program) needs_attention_label: String,
}
impl ExecutionWorkflowPolicy {
	/// Build readiness policy from the registered project workflow.
	pub(crate) fn from_workflow(service_id: &str, workflow: &WorkflowDocument) -> Result<Self> {
		Self::new(
			service_id,
			workflow.frontmatter().tracker().startable_states().to_vec(),
			workflow.frontmatter().tracker().terminal_states().to_vec(),
			workflow.frontmatter().tracker().opt_out_label().to_owned(),
			workflow.frontmatter().tracker().needs_attention_label().to_owned(),
		)
	}

	/// Build readiness policy directly.
	pub(crate) fn new(
		service_id: impl Into<String>,
		startable_states: Vec<String>,
		terminal_states: Vec<String>,
		opt_out_label: impl Into<String>,
		needs_attention_label: impl Into<String>,
	) -> Result<Self> {
		let service_id = service_id.into();
		let policy = Self {
			queue_label: tracker::automation_queue_label(&service_id),
			active_label: tracker::automation_active_label(&service_id),
			service_id,
			startable_states,
			terminal_states,
			opt_out_label: opt_out_label.into(),
			needs_attention_label: needs_attention_label.into(),
		};

		policy.validate()?;

		Ok(policy)
	}

	/// Workflow terminal states.
	pub(crate) fn terminal_states(&self) -> &[String] {
		&self.terminal_states
	}

	pub(in crate::execution_program) fn issue_is_startable(
		&self,
		issue: &ExecutionLinearIssueMapping,
	) -> bool {
		self.startable_states.iter().any(|state| state == issue.issue_state())
	}

	pub(in crate::execution_program) fn issue_is_terminal(
		&self,
		issue: &ExecutionLinearIssueMapping,
	) -> bool {
		self.terminal_states.iter().any(|state| state == issue.issue_state())
	}

	fn validate(&self) -> Result<()> {
		validation::validate_required("execution workflow service_id", &self.service_id)?;
		validation::validate_required("execution workflow queue_label", &self.queue_label)?;
		validation::validate_required("execution workflow active_label", &self.active_label)?;
		validation::validate_required("execution workflow opt_out_label", &self.opt_out_label)?;
		validation::validate_required(
			"execution workflow needs_attention_label",
			&self.needs_attention_label,
		)?;
		validation::validate_string_list(
			"execution workflow startable_states",
			&self.startable_states,
		)?;
		validation::validate_string_list(
			"execution workflow terminal_states",
			&self.terminal_states,
		)?;

		if self.startable_states.is_empty() {
			eyre::bail!("Execution workflow startable_states must not be empty.");
		}
		if self.terminal_states.is_empty() {
			eyre::bail!("Execution workflow terminal_states must not be empty.");
		}

		Ok(())
	}
}
