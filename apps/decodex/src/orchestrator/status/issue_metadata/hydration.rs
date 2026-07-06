use std::{
	collections::{HashMap, HashSet},
	time::Instant,
};

use crate::{
	config::ServiceConfig,
	orchestrator::{
		self, OperatorIssueDisplayMetadata, OperatorStatusSnapshot, RunIssueMetadataHydration,
		TrackerObserverOutcome,
		status_issue_metadata::{apply, missing, selectors},
	},
	tracker::{self, IssueTracker},
	workflow::WorkflowDocument,
};

pub(in crate::orchestrator) fn hydrate_operator_run_rows_from_tracker<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	snapshot: &mut OperatorStatusSnapshot,
	hydration: RunIssueMetadataHydration,
	stale_terminal_local_issue_ids: &HashSet<String>,
) -> TrackerObserverOutcome
where
	T: IssueTracker,
{
	let issue_ids = selectors::operator_snapshot_run_issue_ids(
		snapshot,
		hydration,
		stale_terminal_local_issue_ids,
	);

	if issue_ids.is_empty() {
		return TrackerObserverOutcome::Ok;
	}

	match tracker.refresh_issues(&issue_ids) {
		Ok(issues) => {
			let active_label = tracker::automation_active_label(project.service_id());
			let needs_attention_label =
				workflow.frontmatter().tracker().needs_attention_label().to_owned();
			let metadata_by_issue_id = issues
				.into_iter()
				.map(|issue| {
					let active_label_present = issue.has_label(&active_label);
					let needs_attention_label_present = issue.has_label(&needs_attention_label);

					(
						issue.id,
						OperatorIssueDisplayMetadata {
							issue_identifier: issue.identifier,
							title: Some(issue.title),
							author: issue.author,
							issue_state: Some(issue.state.name),
							active_label_present: Some(active_label_present),
							needs_attention_label_present: Some(needs_attention_label_present),
						},
					)
				})
				.collect::<HashMap<_, _>>();

			apply::hydrate_operator_snapshot_run_rows(snapshot, &metadata_by_issue_id);

			hydrate_missing_current_lane_metadata_or_backoff(
				tracker,
				project,
				snapshot,
				&active_label,
				&needs_attention_label,
			)
		},
		Err(error)
			if issue_ids.iter().any(|issue_id| {
				tracker::issue_lookup_missing_error_for_candidate(&error, issue_id)
			}) =>
		{
			let active_label = tracker::automation_active_label(project.service_id());
			let needs_attention_label =
				workflow.frontmatter().tracker().needs_attention_label().to_owned();

			hydrate_missing_current_lane_metadata_or_backoff(
				tracker,
				project,
				snapshot,
				&active_label,
				&needs_attention_label,
			)
		},
		Err(error) => {
			if let Some(backoff) = orchestrator::tracker_connector_backoff(
				&error,
				Instant::now(),
				"run_issue_metadata",
			) {
				return TrackerObserverOutcome::Backoff(backoff);
			}

			let _ = error;

			tracing::warn!(
				project_id = project.service_id(),
				"Skipped tracker issue metadata hydration for operator run rows; sensitive tracker details were withheld."
			);

			TrackerObserverOutcome::Unavailable
		},
	}
}

fn hydrate_missing_current_lane_metadata_or_backoff<T>(
	tracker: &T,
	project: &ServiceConfig,
	snapshot: &mut OperatorStatusSnapshot,
	active_label: &str,
	needs_attention_label: &str,
) -> TrackerObserverOutcome
where
	T: IssueTracker,
{
	if let Err(error) = missing::hydrate_missing_current_lane_tracker_metadata(
		tracker,
		snapshot,
		active_label,
		needs_attention_label,
	) {
		if let Some(backoff) = orchestrator::tracker_connector_backoff(
			&error,
			Instant::now(),
			"run_issue_identifier_metadata",
		) {
			return TrackerObserverOutcome::Backoff(backoff);
		}

		let _ = error;

		tracing::warn!(
			project_id = project.service_id(),
			"Skipped missing-issue current lane classification; sensitive tracker details were withheld."
		);

		return TrackerObserverOutcome::Unavailable;
	}

	TrackerObserverOutcome::Ok
}
