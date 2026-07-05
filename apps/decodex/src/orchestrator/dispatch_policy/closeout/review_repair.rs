use crate::orchestrator::dispatch_policy::{
	self, IssueTracker, Result, ServiceConfig, TrackerIssue, WorkflowDocument,
};

pub(crate) fn issue_passes_review_repair_dispatch_policy<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	let tracker_policy = workflow.frontmatter().tracker();

	Ok(dispatch_policy::issue_has_service_ownership(tracker, issue, project.service_id())?
		&& issue.state.name == tracker_policy.success_state()
		&& !issue.has_label(tracker_policy.opt_out_label())
		&& !issue.has_label(tracker_policy.needs_attention_label()))
}
