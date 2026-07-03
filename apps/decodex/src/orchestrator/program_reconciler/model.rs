use std::collections::BTreeMap;

use crate::{
	execution_program::{ExecutionLinearIssueMapping, ExecutionProgram},
	orchestrator::{
		ExecutionProgramRecord, IssueTracker, Result, SelectedIssueRunCandidate, StateStore,
		TrackerIssue, WorkflowDocument,
	},
};

pub(crate) const PROGRAM_DISPATCH_SELECTED_SCHEMA: &str = "decodex.program_dispatch_selected/1";
pub(crate) const PROGRAM_DISPATCH_SELECTED_EVENT_TYPE: &str = "program_dispatch_selected";

#[derive(Clone)]
pub(crate) struct RefreshedExecutionProgram {
	pub(crate) record: ExecutionProgramRecord,
	pub(crate) program: ExecutionProgram,
	pub(crate) issues_by_node: BTreeMap<String, ProgramIssueSnapshot>,
}

#[derive(Clone)]
pub(crate) struct ProgramIssueSnapshot {
	pub(crate) issue: TrackerIssue,
	pub(crate) has_active_label: bool,
	pub(crate) has_opt_out_label: bool,
	pub(crate) has_needs_attention_label: bool,
	pub(crate) has_open_tracker_blockers: bool,
	pub(crate) has_generic_dispatch_briefing: bool,
	pub(crate) has_post_review_lifecycle: bool,
}
impl ProgramIssueSnapshot {
	pub(super) fn linear_mapping(&self) -> Result<ExecutionLinearIssueMapping> {
		Ok(ExecutionLinearIssueMapping::new(
			&self.issue.id,
			&self.issue.identifier,
			&self.issue.state.name,
		)?
		.with_active_label(self.has_active_label)
		.with_opt_out_label(self.has_opt_out_label)
		.with_needs_attention_label(self.has_needs_attention_label)
		.with_open_tracker_blockers(self.has_open_tracker_blockers)
		.with_generic_dispatch_briefing(self.has_generic_dispatch_briefing)
		.with_post_review_lifecycle(self.has_post_review_lifecycle))
	}
}

pub(crate) struct ProgramIssueSnapshotInput<'a, T>
where
	T: IssueTracker + ?Sized,
{
	pub(crate) tracker: &'a T,
	pub(crate) state_store: &'a StateStore,
	pub(crate) service_id: &'a str,
	pub(crate) workflow: &'a WorkflowDocument,
	pub(crate) issue: &'a TrackerIssue,
}

#[derive(Default)]
pub(crate) struct ProgramSchedulerSummary {
	pub(crate) programs_evaluated: usize,
	pub(crate) programs_updated: usize,
	pub(crate) dispatchable_nodes: usize,
}

pub(crate) struct ProgramSchedulerSelection {
	pub(crate) selected: Option<SelectedIssueRunCandidate>,
	pub(crate) summary: ProgramSchedulerSummary,
}
