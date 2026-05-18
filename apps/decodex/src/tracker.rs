pub(crate) mod linear;
pub(crate) mod records;

use std::slice;

use color_eyre::Report;

use crate::prelude::{Result, eyre};
use records::LinearExecutionEventRecord;

pub(crate) trait IssueTracker {
	fn list_issues_with_label(&self, label_name: &str) -> Result<Vec<TrackerIssue>>;
	fn find_team_label_id(&self, team_id: &str, label_name: &str) -> Result<Option<String>>;
	fn get_issue_by_identifier(&self, issue_identifier: &str) -> Result<Option<TrackerIssue>>;
	fn refresh_issues(&self, issue_ids: &[String]) -> Result<Vec<TrackerIssue>>;
	fn list_comments(&self, issue_id: &str) -> Result<Vec<TrackerComment>>;
	fn update_issue_state(&self, issue_id: &str, state_id: &str) -> Result<()>;
	fn add_issue_labels(&self, issue_id: &str, label_ids: &[String]) -> Result<()>;
	fn remove_issue_labels(&self, issue_id: &str, label_ids: &[String]) -> Result<()>;
	fn create_comment(&self, issue_id: &str, body: &str) -> Result<()>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrackerComment {
	pub(crate) body: String,
	pub(crate) created_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrackerIssue {
	pub(crate) id: String,
	pub(crate) identifier: String,
	#[cfg(test)]
	pub(crate) project_slug: Option<String>,
	pub(crate) title: String,
	pub(crate) author: Option<String>,
	pub(crate) description: String,
	pub(crate) priority: Option<i64>,
	pub(crate) created_at: String,
	pub(crate) updated_at: String,
	pub(crate) state: TrackerState,
	pub(crate) team: TrackerTeam,
	pub(crate) labels_complete: bool,
	pub(crate) labels: Vec<TrackerLabel>,
	pub(crate) blockers: Vec<TrackerIssueBlocker>,
}
impl TrackerIssue {
	pub(crate) fn has_label(&self, label_name: &str) -> bool {
		self.labels.iter().any(|label| label.name == label_name)
	}

	pub(crate) fn state_id_for_name(&self, state_name: &str) -> Option<&str> {
		self.team
			.states
			.iter()
			.find(|state| state.name == state_name)
			.map(|state| state.id.as_str())
	}

	pub(crate) fn label_id_for_name(&self, label_name: &str) -> Option<&str> {
		self.team
			.labels
			.iter()
			.find(|label| label.name == label_name)
			.map(|label| label.id.as_str())
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrackerIssueBlocker {
	pub(crate) id: String,
	pub(crate) identifier: String,
	pub(crate) state: TrackerState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrackerState {
	pub(crate) id: String,
	pub(crate) name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrackerLabel {
	pub(crate) id: String,
	pub(crate) name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrackerTeam {
	pub(crate) id: String,
	pub(crate) name: String,
	pub(crate) states: Vec<TrackerState>,
	pub(crate) labels: Vec<TrackerLabel>,
}

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
		if label_not_on_issue_error(&error) {
			return Ok(false);
		}

		return Err(error);
	}

	Ok(true)
}

pub(crate) fn label_not_on_issue_error(error: &Report) -> bool {
	error
		.chain()
		.any(|source| source.to_string().to_ascii_lowercase().contains("label not on issue"))
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
			if label_not_on_issue_error(&error) {
				continue;
			}

			return Err(error);
		}
	}

	Ok(())
}

pub(crate) fn create_linear_execution_event_comment<T>(
	tracker: &T,
	issue_id: &str,
	body: &str,
	record: &LinearExecutionEventRecord,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	records::validate_linear_execution_event_record(record).map_err(|error| eyre::eyre!(error))?;

	let comments = tracker.list_comments(issue_id)?;

	if records::has_linear_execution_event_record(
		&comments,
		&record.service_id,
		&record.issue_id,
		&record.idempotency_key,
	) {
		return Ok(false);
	}

	let comment_body = records::append_structured_comment_record(body, record)?;

	tracker.create_comment(issue_id, &comment_body)?;

	Ok(true)
}

pub(crate) fn create_linear_execution_event_comment_without_remote_scan<T>(
	tracker: &T,
	issue_id: &str,
	body: &str,
	record: &LinearExecutionEventRecord,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	records::validate_linear_execution_event_record(record).map_err(|error| eyre::eyre!(error))?;

	let comment_body = records::append_structured_comment_record(body, record)?;

	tracker.create_comment(issue_id, &comment_body)
}
