use crate::orchestrator::daemon_retry::{
	self, CloseoutDispatchEligibility, GhPullRequestReviewStateInspector, IssueTracker, Path,
	Result, RetryEntryLifecycle, ServiceConfig, StateStore, TrackerIssue, WorkflowDocument,
	retention::RetryEntryRetentionDecision,
};

pub(crate) fn evaluate_post_review_retention_policy<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	lifecycle: RetryEntryLifecycle,
) -> Result<RetryEntryRetentionDecision>
where
	T: IssueTracker + ?Sized,
{
	match lifecycle {
		RetryEntryLifecycle::ReviewRepair => Ok(
			if daemon_retry::issue_passes_review_repair_dispatch_policy(
				tracker, issue, project, workflow,
			)? {
				RetryEntryRetentionDecision::Retain
			} else {
				RetryEntryRetentionDecision::Drop
			},
		),
		RetryEntryLifecycle::Closeout => Ok(
			match daemon_retry::evaluate_closeout_dispatch_policy_with_inspector(
				tracker,
				issue,
				project,
				workflow,
				state_store,
				&GhPullRequestReviewStateInspector {
					github_token_env_var: Some(project.github().token_env_var().to_owned()),
					github_command_path: project.github().command_path().map(Path::to_path_buf),
				},
			)? {
				CloseoutDispatchEligibility::Eligible => RetryEntryRetentionDecision::Retain,
				CloseoutDispatchEligibility::Ineligible => RetryEntryRetentionDecision::Drop,
				CloseoutDispatchEligibility::Blocked(_) => RetryEntryRetentionDecision::Block,
			},
		),
		RetryEntryLifecycle::Active => Ok(RetryEntryRetentionDecision::Drop),
	}
}
