use crate::orchestrator::dispatch_policy::{
	GhPullRequestReviewStateInspector, IssueTracker, Path, Result, ServiceConfig, StateStore,
	TrackerIssue, WorkflowDocument, closeout,
};

pub(crate) fn issue_passes_closeout_dispatch_policy<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	let review_state_inspector = review_state_inspector_for_project(project);

	closeout::issue_passes_closeout_dispatch_policy_with_inspector(
		tracker,
		issue,
		project,
		workflow,
		state_store,
		&review_state_inspector,
	)
}

pub(crate) fn closeout_dispatch_block_reason<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<Option<&'static str>>
where
	T: IssueTracker + ?Sized,
{
	let review_state_inspector = review_state_inspector_for_project(project);

	closeout::closeout_dispatch_block_reason_with_inspector(
		tracker,
		issue,
		project,
		workflow,
		state_store,
		&review_state_inspector,
	)
}

fn review_state_inspector_for_project(
	project: &ServiceConfig,
) -> GhPullRequestReviewStateInspector {
	GhPullRequestReviewStateInspector {
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
		github_command_path: project.github().command_path().map(Path::to_path_buf),
	}
}
