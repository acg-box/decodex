use crate::{
	prelude::{Result, eyre},
	recovery::reports::StaleActiveDiagnostic,
	tracker::{IssueTracker, TrackerIssue},
	workflow::WorkflowDocument,
};

pub(super) fn restore_stale_active_startable_state_if_queued<T>(
	tracker: &T,
	workflow: &WorkflowDocument,
	issue: &TrackerIssue,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	if !diagnostic.queue_label_present {
		return Ok(());
	}

	let tracker_policy = workflow.frontmatter().tracker();

	if tracker_policy.startable_states().iter().any(|state| state == &issue.state.name) {
		return Ok(());
	}
	if issue.state.name != tracker_policy.in_progress_state() {
		eyre::bail!(
			"`recover stale-active release` refused `{}` because queued issue state `{}` is not `{}` or a configured startable state.",
			diagnostic.issue_identifier,
			issue.state.name,
			tracker_policy.in_progress_state()
		);
	}

	let startable_state = tracker_policy.startable_states().first().ok_or_else(|| {
		eyre::eyre!("Workflow tracker startable_states must contain at least one state.")
	})?;
	let state_id = issue.state_id_for_name(startable_state).ok_or_else(|| {
		eyre::eyre!(
			"Issue `{}` team does not expose configured startable state `{}`.",
			issue.identifier,
			startable_state
		)
	})?;

	tracker.update_issue_state(&issue.id, state_id)
}
