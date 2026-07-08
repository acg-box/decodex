use crate::orchestrator::daemon_retry::{
	self, CloseoutDispatchEligibility, GhPullRequestReviewStateInspector, IssueTracker, Result,
	RetryEntryLifecycle, ServiceConfig, StateStore, TrackerIssue, WorkflowDocument,
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
		RetryEntryLifecycle::Closeout => {
			let review_state_inspector = GhPullRequestReviewStateInspector::for_project(project);

			Ok(
				match daemon_retry::evaluate_closeout_dispatch_policy_with_inspector(
					tracker,
					issue,
					project,
					workflow,
					state_store,
					&review_state_inspector,
				)? {
					CloseoutDispatchEligibility::Eligible => RetryEntryRetentionDecision::Retain,
					CloseoutDispatchEligibility::Ineligible => RetryEntryRetentionDecision::Drop,
					CloseoutDispatchEligibility::Blocked(_) => RetryEntryRetentionDecision::Block,
				},
			)
		},
		RetryEntryLifecycle::Active => Ok(RetryEntryRetentionDecision::Drop),
	}
}
