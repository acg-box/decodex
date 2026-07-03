use crate::{
	prelude::{Result, eyre},
	recovery::{
		context::RecoveryContext, review_handoff::RebindLabelValidation,
		review_handoff_policy::RebindMode,
	},
	tracker::{self, TrackerIssue},
};

pub(super) fn validate_rebind_tracker_labels(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	mode: RebindMode,
) -> Result<RebindLabelValidation> {
	let active_label = tracker::automation_active_label(context.config.service_id());
	let active_label_present =
		tracker::issue_has_label_with_server_confirmation(&context.tracker, issue, &active_label)?;
	let tracker_policy = context.workflow.frontmatter().tracker();

	if !active_label_present {
		if !mode.allows_failure_state_drift_repair()
			|| issue.state.name != tracker_policy.failure_state()
		{
			eyre::bail!(
				"Issue `{}` is missing active automation label `{active_label}`. Restore explicit lane ownership before rebind.",
				issue.identifier
			);
		}
		if tracker::issue_team_label_id_with_server_confirmation(
			&context.tracker,
			issue,
			&active_label,
		)?
		.is_none()
		{
			eyre::bail!(
				"Issue `{}` is missing active automation label `{active_label}`, but that label was not found on the team.",
				issue.identifier
			);
		}
	}

	let needs_attention_label = tracker_policy.needs_attention_label();
	let needs_attention_present = tracker::issue_has_label_with_server_confirmation(
		&context.tracker,
		issue,
		needs_attention_label,
	)?;

	if !needs_attention_present {
		return Ok(RebindLabelValidation {
			active_label_present,
			restore_active_label: !active_label_present,
			clear_needs_attention_label: false,
		});
	}
	if mode.allows_failure_state_drift_repair()
		&& issue.state.name == tracker_policy.failure_state()
	{
		if tracker::issue_team_label_id_with_server_confirmation(
			&context.tracker,
			issue,
			needs_attention_label,
		)?
		.is_none()
		{
			eyre::bail!(
				"Issue `{}` has needs-attention label `{needs_attention_label}`, but that label was not found on the team.",
				issue.identifier
			);
		}

		return Ok(RebindLabelValidation {
			active_label_present,
			restore_active_label: !active_label_present,
			clear_needs_attention_label: true,
		});
	}

	eyre::bail!(
		"Issue `{}` has needs-attention label `{}`.",
		issue.identifier,
		needs_attention_label
	)
}

pub(super) fn validate_adopt_issue_context(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<RebindLabelValidation> {
	let tracker_policy = context.workflow.frontmatter().tracker();

	if issue.has_label(tracker_policy.opt_out_label()) {
		eyre::bail!(
			"Issue `{}` has opt-out label `{}`.",
			issue.identifier,
			tracker_policy.opt_out_label()
		);
	}

	let active_label = tracker::automation_active_label(context.config.service_id());
	let active_label_present =
		tracker::issue_has_label_with_server_confirmation(&context.tracker, issue, &active_label)?;

	if !active_label_present
		&& tracker::issue_team_label_id_with_server_confirmation(
			&context.tracker,
			issue,
			&active_label,
		)?
		.is_none()
	{
		eyre::bail!(
			"Issue `{}` is missing active automation label `{active_label}`, and that label was not found on the team.",
			issue.identifier
		);
	}

	let needs_attention_label = tracker_policy.needs_attention_label();
	let needs_attention_present = tracker::issue_has_label_with_server_confirmation(
		&context.tracker,
		issue,
		needs_attention_label,
	)?;

	if needs_attention_present {
		eyre::bail!(
			"Issue `{}` has needs-attention label `{needs_attention_label}`; manual takeover adopt will not bypass a human-required stop.",
			issue.identifier
		);
	}

	Ok(RebindLabelValidation {
		active_label_present,
		restore_active_label: false,
		clear_needs_attention_label: false,
	})
}
