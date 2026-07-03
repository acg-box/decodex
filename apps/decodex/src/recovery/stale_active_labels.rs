//! Tracker label and shared-claim inspection for stale-active recovery.

use crate::{
	prelude::Result,
	recovery::reports::StaleActiveDiagnostic,
	state::StateStore,
	tracker::{self, IssueTracker, TrackerIssue},
	workflow::WorkflowDocument,
};

pub(super) struct StaleActiveLabelSnapshot {
	pub(super) queue_label_present: bool,
	pub(super) active_label_present: bool,
	pub(super) needs_attention_label_present: bool,
}

pub(super) fn stale_active_tracker_issue_keys(issue: &TrackerIssue) -> Vec<String> {
	stale_active_issue_keys(&issue.id, &issue.identifier)
}

pub(super) fn stale_active_diagnostic_issue_keys(
	diagnostic: &StaleActiveDiagnostic,
) -> Vec<String> {
	stale_active_issue_keys(&diagnostic.issue_id, &diagnostic.issue_identifier)
}

pub(super) fn inspect_stale_active_labels<T>(
	project_id: &str,
	workflow: &WorkflowDocument,
	tracker: &T,
	issue: &TrackerIssue,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<StaleActiveLabelSnapshot>
where
	T: IssueTracker + ?Sized,
{
	let active_label = tracker::automation_active_label(project_id);
	let queue_label = tracker::automation_queue_label(project_id);
	let needs_attention_label = workflow.frontmatter().tracker().needs_attention_label();
	let active_label_present =
		tracker::issue_has_label_with_server_confirmation(tracker, issue, &active_label)?;
	let queue_label_present =
		tracker::issue_has_label_with_server_confirmation(tracker, issue, &queue_label)?;
	let needs_attention_label_present =
		tracker::issue_has_label_with_server_confirmation(tracker, issue, needs_attention_label)?;

	if active_label_present {
		evidence.push(String::from("active_label_present"));
	} else {
		blockers.push(String::from("active_label_missing"));
	}
	if queue_label_present {
		evidence.push(String::from("queue_label_present"));
	} else {
		evidence.push(String::from("queue_label_missing"));
	}
	if needs_attention_label_present {
		blockers.push(String::from("needs_attention_label_present"));
	} else {
		evidence.push(String::from("needs_attention_label_missing"));
	}

	Ok(StaleActiveLabelSnapshot {
		queue_label_present,
		active_label_present,
		needs_attention_label_present,
	})
}

pub(super) fn inspect_stale_active_shared_claim(
	project_id: &str,
	state_store: &StateStore,
	issue_keys: &[String],
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> bool {
	let active_shared_claim =
		match stale_active_issue_has_active_shared_claim(project_id, state_store, issue_keys) {
			Ok(active_shared_claim) => active_shared_claim,
			Err(error) => {
				blockers.push(String::from("active_shared_claim_unknown"));
				evidence.push(format!("active_shared_claim_error:{}", error));

				false
			},
		};

	if active_shared_claim {
		blockers.push(String::from("active_shared_claim_present"));
	} else if !blockers.iter().any(|blocker| blocker == "active_shared_claim_unknown") {
		evidence.push(String::from("active_shared_claim_missing"));
	}

	active_shared_claim
}

pub(super) fn stale_active_issue_has_active_shared_claim(
	project_id: &str,
	state_store: &StateStore,
	issue_keys: &[String],
) -> Result<bool> {
	for issue_key in issue_keys {
		if state_store.issue_has_active_shared_claim_read_only(project_id, issue_key)? {
			return Ok(true);
		}
	}

	Ok(false)
}

fn stale_active_issue_keys(issue_id: &str, issue_identifier: &str) -> Vec<String> {
	let mut keys = vec![issue_id.to_owned()];

	if issue_identifier != issue_id {
		keys.push(issue_identifier.to_owned());
	}

	keys
}
