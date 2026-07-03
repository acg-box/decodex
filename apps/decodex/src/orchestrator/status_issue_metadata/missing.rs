use std::collections::HashMap;

use crate::{
	orchestrator::{
		self, OperatorIssueDisplayMetadata, OperatorStatusSnapshot,
		status_issue_metadata::{apply, selectors},
	},
	prelude::Result,
	tracker::{self, IssueTracker, TrackerIssue},
};

pub(in crate::orchestrator::status_issue_metadata) fn hydrate_missing_current_lane_tracker_metadata<
	T,
>(
	tracker: &T,
	snapshot: &mut OperatorStatusSnapshot,
	active_label: &str,
	needs_attention_label: &str,
) -> Result<()>
where
	T: IssueTracker,
{
	let mut metadata_by_issue_id = HashMap::new();
	let mut missing_rows = Vec::new();

	for run in &snapshot.current_lanes {
		if run.issue_state.is_some() {
			continue;
		}

		let Some(selector) = selectors::operator_run_tracker_issue_identifier_selector(run) else {
			continue;
		};

		match tracker.get_issue_by_identifier(&selector) {
			Ok(Some(issue)) => {
				metadata_by_issue_id.insert(
					run.issue_id.clone(),
					operator_issue_display_metadata(&issue, active_label, needs_attention_label),
				);
			},
			Ok(None) => missing_rows.push((run.run_id.clone(), run.issue_id.clone(), selector)),
			Err(error) if tracker::issue_lookup_missing_error_for_candidate(&error, &selector) => {
				missing_rows.push((run.run_id.clone(), run.issue_id.clone(), selector));
			},
			Err(error) => return Err(error),
		}
	}

	if !metadata_by_issue_id.is_empty() {
		apply::hydrate_operator_snapshot_run_rows(snapshot, &metadata_by_issue_id);
	}

	for (run_id, issue_id, selector) in missing_rows {
		orchestrator::mark_operator_run_tracker_issue_missing(
			snapshot, &run_id, &issue_id, &selector,
		);
	}

	Ok(())
}

fn operator_issue_display_metadata(
	issue: &TrackerIssue,
	active_label: &str,
	needs_attention_label: &str,
) -> OperatorIssueDisplayMetadata {
	OperatorIssueDisplayMetadata {
		issue_identifier: issue.identifier.clone(),
		title: Some(issue.title.clone()),
		author: issue.author.clone(),
		issue_state: Some(issue.state.name.clone()),
		active_label_present: Some(issue.has_label(active_label)),
		needs_attention_label_present: Some(issue.has_label(needs_attention_label)),
	}
}
