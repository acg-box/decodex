//! Private evidence and review authority checks for stale-active recovery.

use crate::{
	prelude::{Result, eyre},
	state::{ProjectRunStatus, StateStore},
	tracker::{
		IssueTracker, TrackerIssue,
		records::{self, LinearExecutionEventRecord},
	},
};

use super::{
	evidence::{
		ghost_lane_record_has_pr_or_review_lineage, stale_active_private_event_allows_release,
		stale_active_private_event_is_release_audit_for_run,
	},
	process_liveness::StaleActiveProcessLiveness,
	reports::StaleActiveDiagnostic,
};

pub(super) fn inspect_stale_active_private_evidence(
	project_id: &str,
	state_store: &StateStore,
	issue_keys: &[String],
	latest_run: Option<&ProjectRunStatus>,
	marker_liveness: StaleActiveProcessLiveness,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()> {
	let mut events = Vec::new();

	for issue_key in issue_keys {
		events.extend(state_store.list_private_execution_events_for_issue(project_id, issue_key)?);
	}

	if events.is_empty() {
		evidence.push(String::from("private_evidence_missing"));
	} else {
		let release_audit_present = events
			.iter()
			.any(|event| stale_active_private_event_is_release_audit_for_run(event, latest_run));
		if release_audit_present {
			evidence.push(String::from("stale_active_release_audit_present"));
		}
		if events.iter().all(|event| {
			stale_active_private_event_allows_release(event, marker_liveness, release_audit_present)
		}) {
			evidence.push(String::from("only_stale_active_or_failed_control_evidence_present"));
		} else {
			blockers.push(String::from("private_progress_evidence_present"));
		}
	}

	Ok(())
}

pub(super) fn inspect_stale_active_review_lineage<T>(
	project_id: &str,
	state_store: &StateStore,
	tracker: &T,
	issue: &TrackerIssue,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	if state_store.issue_has_review_lifecycle_record(project_id, &issue.id)?
		|| (issue.identifier != issue.id
			&& state_store.issue_has_review_lifecycle_record(project_id, &issue.identifier)?)
	{
		blockers.push(String::from("review_lifecycle_present"));

		return Ok(());
	}
	if state_store.issue_has_review_policy_checkpoint(project_id, &issue.id)?
		|| (issue.identifier != issue.id
			&& state_store.issue_has_review_policy_checkpoint(project_id, &issue.identifier)?)
	{
		blockers.push(String::from("review_policy_checkpoint_present"));

		return Ok(());
	}

	let records = stale_active_review_lineage_records(
		project_id,
		state_store,
		tracker,
		&issue.id,
		&issue.identifier,
	)?;

	if records.iter().any(ghost_lane_record_has_pr_or_review_lineage) {
		blockers.push(String::from("pr_or_review_lineage_present"));
	} else {
		evidence.push(String::from("review_lineage_missing"));
	}

	Ok(())
}

pub(super) fn ensure_stale_active_review_authority_missing<T>(
	tracker: &T,
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let mut blockers = Vec::new();

	if state_store
		.issue_has_review_lifecycle_record(&diagnostic.project_id, &diagnostic.issue_id)?
		|| (diagnostic.issue_identifier != diagnostic.issue_id
			&& state_store.issue_has_review_lifecycle_record(
				&diagnostic.project_id,
				&diagnostic.issue_identifier,
			)?) {
		blockers.push("review_lifecycle_present");
	}
	if state_store
		.issue_has_review_policy_checkpoint(&diagnostic.project_id, &diagnostic.issue_id)?
		|| (diagnostic.issue_identifier != diagnostic.issue_id
			&& state_store.issue_has_review_policy_checkpoint(
				&diagnostic.project_id,
				&diagnostic.issue_identifier,
			)?) {
		blockers.push("review_policy_checkpoint_present");
	}

	let records = stale_active_review_lineage_records(
		&diagnostic.project_id,
		state_store,
		tracker,
		&diagnostic.issue_id,
		&diagnostic.issue_identifier,
	)?;
	if records.iter().any(ghost_lane_record_has_pr_or_review_lineage) {
		blockers.push("pr_or_review_lineage_present");
	}

	if blockers.is_empty() {
		return Ok(());
	}

	eyre::bail!(
		"`recover stale-active release` refused `{}` because review authority appeared before active-label release: {}",
		diagnostic.issue_identifier,
		blockers.join(", ")
	)
}

fn stale_active_review_lineage_records<T>(
	project_id: &str,
	state_store: &StateStore,
	tracker: &T,
	issue_id: &str,
	issue_identifier: &str,
) -> Result<Vec<LinearExecutionEventRecord>>
where
	T: IssueTracker + ?Sized,
{
	let mut records = state_store.list_linear_execution_events(project_id, issue_id)?;

	if issue_identifier != issue_id {
		records.extend(state_store.list_linear_execution_events(project_id, issue_identifier)?);
	}

	let comments = tracker.list_comments(issue_id)?;

	records.extend(comments.iter().filter_map(|comment| {
		records::parse_linear_execution_event_record(&comment.body).filter(|record| {
			record.service_id == project_id
				&& (record.issue_id == issue_id
					|| record.issue_id == issue_identifier
					|| record.issue_identifier == issue_identifier
					|| record.issue_identifier == issue_id)
		})
	}));

	Ok(records)
}
