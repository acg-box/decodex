//! Workflow policy and runtime observations for execution-program readiness.

use std::collections::BTreeMap;

use super::{
	model::{ExecutionConflictDomain, ExecutionLinearIssueMapping, ExecutionQueueIntent},
	validation::{validate_optional, validate_required, validate_string_list},
};
use crate::{
	prelude::{Result, eyre},
	tracker,
	workflow::WorkflowDocument,
};

/// Workflow policy needed for Execution Program readiness.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionWorkflowPolicy {
	pub(super) service_id: String,
	queue_label: String,
	pub(super) active_label: String,
	startable_states: Vec<String>,
	terminal_states: Vec<String>,
	pub(super) opt_out_label: String,
	pub(super) needs_attention_label: String,
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

	/// Service-scoped queue label.
	pub(crate) fn queue_label(&self) -> &str {
		&self.queue_label
	}

	/// Workflow terminal states.
	pub(crate) fn terminal_states(&self) -> &[String] {
		&self.terminal_states
	}

	pub(super) fn issue_is_startable(&self, issue: &ExecutionLinearIssueMapping) -> bool {
		self.startable_states.iter().any(|state| state == issue.issue_state())
	}

	pub(super) fn issue_is_terminal(&self, issue: &ExecutionLinearIssueMapping) -> bool {
		self.terminal_states.iter().any(|state| state == issue.issue_state())
	}

	fn validate(&self) -> Result<()> {
		validate_required("execution workflow service_id", &self.service_id)?;
		validate_required("execution workflow queue_label", &self.queue_label)?;
		validate_required("execution workflow active_label", &self.active_label)?;
		validate_required("execution workflow opt_out_label", &self.opt_out_label)?;
		validate_required("execution workflow needs_attention_label", &self.needs_attention_label)?;
		validate_string_list("execution workflow startable_states", &self.startable_states)?;
		validate_string_list("execution workflow terminal_states", &self.terminal_states)?;

		if self.startable_states.is_empty() {
			eyre::bail!("Execution workflow startable_states must not be empty.");
		}
		if self.terminal_states.is_empty() {
			eyre::bail!("Execution workflow terminal_states must not be empty.");
		}

		Ok(())
	}
}

/// Runtime dependency observation used by readiness evaluation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionDependencySnapshot {
	pub(super) dependency_id: String,
	pub(super) tracker_state: Option<String>,
	pub(super) queue_intent: Option<ExecutionQueueIntent>,
}
impl ExecutionDependencySnapshot {
	/// Observe a dependency through a tracker state.
	pub(crate) fn tracker_state(
		dependency_id: impl Into<String>,
		state: impl Into<String>,
	) -> Result<Self> {
		let snapshot = Self {
			dependency_id: dependency_id.into(),
			tracker_state: Some(state.into()),
			queue_intent: None,
		};

		snapshot.validate()?;

		Ok(snapshot)
	}

	/// Observe a dependency through another internal node dispatch intent.
	pub(crate) fn queue_intent(
		dependency_id: impl Into<String>,
		queue_intent: ExecutionQueueIntent,
	) -> Result<Self> {
		let snapshot = Self {
			dependency_id: dependency_id.into(),
			tracker_state: None,
			queue_intent: Some(queue_intent),
		};

		snapshot.validate()?;

		Ok(snapshot)
	}

	fn validate(&self) -> Result<()> {
		validate_required("execution dependency snapshot.dependency_id", &self.dependency_id)?;

		validate_optional(
			"execution dependency snapshot.tracker_state",
			self.tracker_state.as_deref(),
		)
	}
}

/// Runtime context supplied to readiness evaluation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ExecutionProgramReadinessContext {
	dependency_snapshots: Vec<ExecutionDependencySnapshot>,
	pub(super) occupied_conflict_domains: Vec<ExecutionConflictDomain>,
	pub(super) active_issue_ids: Vec<String>,
}
impl ExecutionProgramReadinessContext {
	/// Build an empty readiness context.
	pub(crate) fn new() -> Self {
		Self::default()
	}

	/// Add dependency observations.
	pub(crate) fn with_dependency_snapshots(
		mut self,
		snapshots: impl IntoIterator<Item = ExecutionDependencySnapshot>,
	) -> Self {
		self.dependency_snapshots = snapshots.into_iter().collect();

		self
	}

	/// Add conflict domains already occupied by active or retained work.
	pub(crate) fn with_occupied_conflict_domains(
		mut self,
		domains: impl IntoIterator<Item = ExecutionConflictDomain>,
	) -> Self {
		self.occupied_conflict_domains = domains.into_iter().collect();

		self
	}

	/// Add mapped Linear issues already owned by a live run claim.
	pub(crate) fn with_active_issue_ids(
		mut self,
		issue_ids: impl IntoIterator<Item = impl Into<String>>,
	) -> Self {
		self.active_issue_ids = issue_ids.into_iter().map(Into::into).collect();

		self
	}

	pub(super) fn dependency_lookup(&self) -> BTreeMap<&str, &ExecutionDependencySnapshot> {
		self.dependency_snapshots
			.iter()
			.map(|snapshot| (snapshot.dependency_id.as_str(), snapshot))
			.collect()
	}
}
