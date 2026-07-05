use crate::{
	orchestrator,
	prelude::Result,
	program_intake::{issue_batch::nodes::intent, model::IssueFacts},
	tracker::{self, IssueTracker, TrackerIssue},
	workflow::WorkflowDocument,
};

pub(in crate::program_intake) fn issue_facts<T>(
	tracker: &T,
	workflow: &WorkflowDocument,
	issue: &TrackerIssue,
	active_label: &str,
) -> Result<IssueFacts>
where
	T: IssueTracker + ?Sized,
{
	let tracker_policy = workflow.frontmatter().tracker();
	let has_active_label =
		tracker::issue_has_label_with_server_confirmation(tracker, issue, active_label)?;
	let has_opt_out_label = tracker::issue_has_label_with_server_confirmation(
		tracker,
		issue,
		tracker_policy.opt_out_label(),
	)?;
	let has_needs_attention_label = tracker::issue_has_label_with_server_confirmation(
		tracker,
		issue,
		tracker_policy.needs_attention_label(),
	)?;
	let has_open_blockers = issue
		.blockers
		.iter()
		.any(|blocker| !intent::state_name_is_terminal(&blocker.state.name, workflow));

	Ok(IssueFacts {
		has_active_label,
		has_opt_out_label,
		has_needs_attention_label,
		has_generic_dispatch_briefing: issue_has_generic_dispatch_briefing(issue),
		has_open_blockers,
	})
}

pub(in crate::program_intake) fn issue_has_generic_dispatch_briefing(issue: &TrackerIssue) -> bool {
	orchestrator::issue_has_generic_dispatch_briefing(issue)
}
