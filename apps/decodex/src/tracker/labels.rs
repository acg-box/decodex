use std::slice;

use crate::{
	prelude::{Result, eyre},
	tracker::{
		errors,
		types::{IssueTracker, TrackerIssue},
	},
};

pub(crate) fn automation_queue_label(service_id: &str) -> String {
	format!("decodex:queued:{service_id}")
}

pub(crate) fn automation_active_label(service_id: &str) -> String {
	format!("decodex:active:{service_id}")
}

pub(crate) fn issue_has_label_with_server_confirmation<T>(
	tracker: &T,
	issue: &TrackerIssue,
	label_name: &str,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	if issue.has_label(label_name) {
		return Ok(true);
	}
	if issue.labels_complete {
		return Ok(false);
	}

	Ok(tracker
		.list_issues_with_label(label_name)?
		.into_iter()
		.any(|candidate| candidate.id == issue.id))
}

pub(crate) fn issue_team_label_id_with_server_confirmation<T>(
	tracker: &T,
	issue: &TrackerIssue,
	label_name: &str,
) -> Result<Option<String>>
where
	T: IssueTracker + ?Sized,
{
	if let Some(label_id) = issue.label_id_for_name(label_name) {
		return Ok(Some(label_id.to_owned()));
	}

	tracker.find_team_label_id(&issue.team.id, label_name)
}

pub(crate) fn set_issue_label_presence<T>(
	tracker: &T,
	issue: &TrackerIssue,
	label_name: &str,
	present: bool,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	let label_present = issue_has_label_with_server_confirmation(tracker, issue, label_name)?;

	if label_present == present {
		return Ok(false);
	}

	let Some(label_id) = issue_team_label_id_with_server_confirmation(tracker, issue, label_name)?
	else {
		eyre::bail!(
			"Issue `{}` does not expose required label `{}` on its team.",
			issue.identifier,
			label_name
		);
	};

	if present {
		tracker.add_issue_labels(&issue.id, &[label_id])?;
	} else if let Err(error) = tracker.remove_issue_labels(&issue.id, &[label_id]) {
		if errors::label_not_on_issue_error(&error) {
			return Ok(false);
		}

		return Err(error);
	}

	Ok(true)
}

pub(crate) fn clear_automation_lane_labels<T>(
	tracker: &T,
	issue: &TrackerIssue,
	service_id: &str,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let mut refreshed_issues = tracker.refresh_issues(slice::from_ref(&issue.id))?;
	let current_issue = refreshed_issues.pop().unwrap_or_else(|| issue.clone());
	let lane_labels = [automation_active_label(service_id), automation_queue_label(service_id)];

	for label_name in lane_labels {
		if !issue_has_label_with_server_confirmation(tracker, &current_issue, &label_name)? {
			continue;
		}

		let Some(label_id) =
			issue_team_label_id_with_server_confirmation(tracker, &current_issue, &label_name)?
		else {
			eyre::bail!(
				"Issue `{}` does not expose required label `{}` on its team.",
				current_issue.identifier,
				label_name
			);
		};

		if let Err(error) = tracker.remove_issue_labels(&current_issue.id, &[label_id]) {
			if errors::label_not_on_issue_error(&error) {
				continue;
			}

			return Err(error);
		}
	}

	Ok(())
}
