use crate::{
	prelude::{Result, eyre},
	recovery::review_handoff_policy::{RebindMode, RebindSuccessStateTransition},
	tracker::TrackerIssue,
	workflow::WorkflowTracker,
};

pub(in crate::recovery) fn validate_rebind_issue_state_for_policy(
	tracker_policy: &WorkflowTracker,
	issue: &TrackerIssue,
	mode: RebindMode,
) -> Result<Option<RebindSuccessStateTransition>> {
	let success_state = tracker_policy.success_state();

	if issue.state.name == success_state {
		return Ok(None);
	}
	if mode.allows_partial_handoff_state_completion()
		&& issue.state.name == tracker_policy.in_progress_state()
	{
		let state_id = issue.state_id_for_name(success_state).ok_or_else(|| {
			eyre::eyre!("State `{success_state}` was not found for issue `{}`.", issue.identifier)
		})?;

		return Ok(Some(RebindSuccessStateTransition {
			state_name: success_state.to_owned(),
			state_id: state_id.to_owned(),
		}));
	}
	if mode.allows_failure_state_drift_repair()
		&& issue.state.name == tracker_policy.failure_state()
	{
		let state_id = issue.state_id_for_name(success_state).ok_or_else(|| {
			eyre::eyre!("State `{success_state}` was not found for issue `{}`.", issue.identifier)
		})?;

		return Ok(Some(RebindSuccessStateTransition {
			state_name: success_state.to_owned(),
			state_id: state_id.to_owned(),
		}));
	}

	eyre::bail!(
		"Issue `{}` is in `{}`, but review handoff rebind requires `{}`{}.",
		issue.identifier,
		issue.state.name,
		success_state,
		if mode.allows_partial_handoff_state_completion() {
			format!(
				" or `{}`{} for a partial handoff recovery",
				tracker_policy.in_progress_state(),
				if mode.allows_failure_state_drift_repair() {
					format!(" or `{}` for state drift recovery", tracker_policy.failure_state())
				} else {
					String::new()
				}
			)
		} else {
			String::new()
		}
	)
}

pub(in crate::recovery) fn validate_adopt_issue_state_for_policy(
	tracker_policy: &WorkflowTracker,
	issue: &TrackerIssue,
) -> Result<Option<RebindSuccessStateTransition>> {
	let success_state = tracker_policy.success_state();

	if issue.state.name == success_state {
		return Ok(None);
	}
	if issue.state.name == tracker_policy.in_progress_state() {
		let state_id = issue.state_id_for_name(success_state).ok_or_else(|| {
			eyre::eyre!("State `{success_state}` was not found for issue `{}`.", issue.identifier)
		})?;

		return Ok(Some(RebindSuccessStateTransition {
			state_name: success_state.to_owned(),
			state_id: state_id.to_owned(),
		}));
	}

	eyre::bail!(
		"Issue `{}` is in `{}`, but manual takeover adopt requires `{}` or `{}`.",
		issue.identifier,
		issue.state.name,
		tracker_policy.in_progress_state(),
		success_state
	)
}
