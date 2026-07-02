use std::collections::BTreeMap;

use crate::{
	execution_program::{ExecutionLinearIssueMapping, ExecutionProgram},
	orchestrator::{
		ExecutionProgramRecord, IssueTracker, Result, SelectedIssueRunCandidate, StateStore,
		TrackerIssue, WorkflowDocument,
	},
};

pub(in crate::orchestrator) const PROGRAM_DISPATCH_SELECTED_SCHEMA: &str =
	"decodex.program_dispatch_selected/1";
pub(in crate::orchestrator) const PROGRAM_DISPATCH_SELECTED_EVENT_TYPE: &str =
	"program_dispatch_selected";

#[derive(Clone)]
pub(in crate::orchestrator) struct RefreshedExecutionProgram {
	pub(in crate::orchestrator) record: ExecutionProgramRecord,
	pub(in crate::orchestrator) program: ExecutionProgram,
	pub(in crate::orchestrator) issues_by_node: BTreeMap<String, ProgramIssueSnapshot>,
}

#[derive(Clone)]
pub(in crate::orchestrator) struct ProgramIssueSnapshot {
	pub(in crate::orchestrator) issue: TrackerIssue,
	pub(in crate::orchestrator) has_active_label: bool,
	pub(in crate::orchestrator) has_opt_out_label: bool,
	pub(in crate::orchestrator) has_needs_attention_label: bool,
	pub(in crate::orchestrator) has_open_tracker_blockers: bool,
	pub(in crate::orchestrator) has_generic_dispatch_briefing: bool,
	pub(in crate::orchestrator) has_post_review_lifecycle: bool,
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

pub(in crate::orchestrator) struct ProgramIssueSnapshotInput<'a, T>
where
	T: IssueTracker + ?Sized,
{
	pub(in crate::orchestrator) tracker: &'a T,
	pub(in crate::orchestrator) state_store: &'a StateStore,
	pub(in crate::orchestrator) service_id: &'a str,
	pub(in crate::orchestrator) workflow: &'a WorkflowDocument,
	pub(in crate::orchestrator) issue: &'a TrackerIssue,
}

#[derive(Default)]
pub(in crate::orchestrator) struct ProgramSchedulerSummary {
	pub(in crate::orchestrator) programs_evaluated: usize,
	pub(in crate::orchestrator) programs_updated: usize,
	pub(in crate::orchestrator) dispatchable_nodes: usize,
}

pub(in crate::orchestrator) struct ProgramSchedulerSelection {
	pub(in crate::orchestrator) selected: Option<SelectedIssueRunCandidate>,
	pub(in crate::orchestrator) summary: ProgramSchedulerSummary,
}
