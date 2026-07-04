mod developer_instructions;
mod prompting_contracts;
mod prompting_recovery;
mod prompting_review_context;
mod prompting_review_guidance;
mod prompting_workflow_context;
mod user_input;

pub(crate) use prompting_workflow_context::review_pull_request_title;

use crate::orchestrator::{
	IssueDispatchMode, IssueRunPlan, IssueTracker, Result, ReviewHandoffContext, ReviewLevel,
	ServiceConfig, StateStore, TrackerIssue, WorkflowDocument,
};

pub(crate) const TRACKER_PUBLIC_TEXT_BOUNDARY_INSTRUCTION: &str =
	prompting_contracts::TRACKER_PUBLIC_TEXT_BOUNDARY_INSTRUCTION;
pub(crate) const DOCS_IMPACT_CONTRACT: &str = prompting_contracts::DOCS_IMPACT_CONTRACT;

pub(crate) fn build_review_run_context(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<ReviewHandoffContext> {
	prompting_review_context::build_review_run_context(project, state_store, issue_run)
}

pub(crate) fn validate_workflow_read_first_files(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
) -> Result<()> {
	prompting_workflow_context::validate_workflow_read_first_files(project, workflow)
}

pub(crate) fn build_developer_instructions<T>(
	_tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	state_store: &StateStore,
	recorded_pr_url: Option<&str>,
) -> Result<String>
where
	T: IssueTracker + ?Sized,
{
	developer_instructions::build_developer_instructions(
		_tracker,
		project,
		workflow,
		issue_run,
		state_store,
		recorded_pr_url,
	)
}

pub(crate) fn build_user_input<T>(
	_tracker: &T,
	project: &ServiceConfig,
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	state_store: &StateStore,
	recorded_pr_url: Option<&str>,
) -> String
where
	T: IssueTracker + ?Sized,
{
	user_input::build_user_input(
		_tracker,
		project,
		issue,
		workflow,
		issue_run,
		state_store,
		recorded_pr_url,
	)
}

pub(crate) fn build_continuation_user_input(
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
	dispatch_mode: IssueDispatchMode,
	recorded_pr_url: Option<&str>,
	success_state: &str,
	review_level: ReviewLevel,
) -> String {
	user_input::build_continuation_user_input(
		issue,
		workflow,
		dispatch_mode,
		recorded_pr_url,
		success_state,
		review_level,
	)
}
