use crate::orchestrator::execution::{
	self, DecodexRunContext, IssueRunPlan, IssueTracker, Path, RUN_OPERATION_GIT_CREDENTIALS,
	Result, ReviewHandoffContext, ServiceConfig, StateStore, WorkflowDocument, state,
};

pub(super) fn build_run_developer_instructions<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	review_context: &ReviewHandoffContext,
) -> Result<String>
where
	T: IssueTracker,
{
	execution::build_developer_instructions(
		tracker,
		project,
		workflow,
		issue_run,
		state_store,
		review_context.recorded_pr_url.as_deref(),
	)
}

pub(super) fn build_run_user_input<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	review_context: &ReviewHandoffContext,
) -> String
where
	T: IssueTracker,
{
	execution::build_user_input(
		tracker,
		project,
		&issue_run.issue,
		workflow,
		issue_run,
		state_store,
		review_context.recorded_pr_url.as_deref(),
	)
}

pub(super) fn build_decodex_run_context(
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
) -> DecodexRunContext {
	let execution = workflow.frontmatter().execution();

	DecodexRunContext {
		run_id: issue_run.run_id.clone(),
		attempt_number: issue_run.attempt_number,
		issue_id: issue_run.issue.id.clone(),
		issue_identifier: issue_run.issue.identifier.clone(),
		branch: issue_run.worktree.branch_name.clone(),
		worktree_path: issue_run.worktree.path.display().to_string(),
		max_turns: execution.max_turns(),
		default_canonicalize_commands: execution.canonicalize_commands().to_vec(),
		default_verify_commands: execution.verify_commands().to_vec(),
	}
}

pub(super) fn write_git_credentials_operation_marker(issue_run: &IssueRunPlan) {
	write_run_operation_marker_best_effort(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		RUN_OPERATION_GIT_CREDENTIALS,
	);
}

pub(crate) fn write_run_operation_marker_best_effort(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	current_operation: &str,
) {
	if let Err(error) =
		state::write_run_operation_marker(worktree_path, run_id, attempt_number, current_operation)
	{
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			current_operation,
			worktree_path = %worktree_path.display(),
			"Run operation marker write failed; continuing completion flow."
		);
	}
}
