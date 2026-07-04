use color_eyre::{Report, eyre::WrapErr};

use crate::{
	prelude::{Result, eyre},
	state::StateStore,
	tracker::{self, IssueTracker, TrackerIssue},
};

pub(in crate::manual) fn clear_manual_closeout_issue_scope<T>(
	tracker: &T,
	issue: &TrackerIssue,
	service_id: &str,
	needs_attention_label: &str,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let closeout_labels = [
		tracker::automation_active_label(service_id),
		tracker::automation_queue_label(service_id),
		needs_attention_label.to_owned(),
	];

	for label_name in closeout_labels {
		clear_manual_closeout_issue_label(tracker, issue, &label_name)?;
	}

	Ok(())
}

pub(in crate::manual) fn clear_manual_closeout_runtime_state(
	state_store: &StateStore,
	issue_id: &str,
	handoff_run_id: &str,
) -> Result<()> {
	state_store.succeed_running_run_attempts_for_issue(issue_id).wrap_err_with(|| {
		format!("Failed to finalize running runtime attempts for issue `{issue_id}`.")
	})?;

	succeed_manual_land_handoff_attempt(state_store, issue_id, handoff_run_id)?;

	state_store
		.clear_lease(issue_id)
		.wrap_err_with(|| format!("Failed to clear runtime lease for issue `{issue_id}`."))?;
	state_store.clear_worktree(issue_id).wrap_err_with(|| {
		format!("Failed to clear runtime worktree state for issue `{issue_id}`.")
	})?;

	Ok(())
}

pub(in crate::manual) fn succeed_manual_land_handoff_attempt(
	state_store: &StateStore,
	issue_id: &str,
	handoff_run_id: &str,
) -> Result<()> {
	let Some(attempt) = state_store.run_attempt(handoff_run_id)? else {
		return Ok(());
	};

	if attempt.issue_id() != issue_id {
		eyre::bail!(
			"Manual land handoff run `{handoff_run_id}` belongs to issue `{}`, not `{issue_id}`.",
			attempt.issue_id()
		);
	}
	if attempt.status() != "succeeded" {
		state_store.update_run_status(handoff_run_id, "succeeded")?;
	}

	Ok(())
}

fn clear_manual_closeout_issue_label<T>(
	tracker: &T,
	issue: &TrackerIssue,
	label_name: &str,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	if let Err(error) = tracker::set_issue_label_presence(tracker, issue, label_name, false)
		&& !linear_label_not_on_issue_error(&error)
	{
		return Err(error);
	}

	Ok(())
}

fn linear_label_not_on_issue_error(error: &Report) -> bool {
	error
		.chain()
		.any(|source| source.to_string().to_ascii_lowercase().contains("label not on issue"))
}
