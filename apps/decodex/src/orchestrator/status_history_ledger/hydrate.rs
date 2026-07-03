use std::{collections::HashSet, time::Instant};

use crate::{
	config::ServiceConfig,
	orchestrator::{
		self, OperatorHistoryLaneStatus, OperatorHistoryLedgerRecord, OperatorIssueDisplayMetadata,
		OperatorStatusSnapshot, TrackerObserverOutcome,
		status_history_ledger::{outcome, records},
	},
	tracker::IssueTracker,
};

pub(crate) fn hydrate_history_lanes_from_linear_ledger<T>(
	tracker: &T,
	project: &ServiceConfig,
	snapshot: &mut OperatorStatusSnapshot,
	stale_terminal_local_issue_ids: &HashSet<String>,
) -> TrackerObserverOutcome
where
	T: IssueTracker,
{
	let mut unavailable = false;

	for lane in &mut snapshot.history_lanes {
		if orchestrator::operator_run_is_stale_terminal_local_residue(
			&lane.latest_run,
			stale_terminal_local_issue_ids,
		) {
			lane.ledger_outcome = outcome::stale_terminal_local_history_ledger_outcome();

			continue;
		}

		match tracker.list_comments(&lane.issue_id) {
			Ok(comments) => {
				let records = records::collect_history_ledger_records(
					project.service_id(),
					&lane.issue_id,
					&comments,
				);

				hydrate_history_lane_from_ledger_records(lane, &records);

				lane.ledger_outcome = outcome::operator_history_ledger_outcome(&records);
			},
			Err(error) => {
				if let Some(backoff) = orchestrator::tracker_connector_backoff(
					&error,
					Instant::now(),
					"execution_ledger_status",
				) {
					lane.ledger_outcome = outcome::unavailable_history_ledger_outcome();

					return TrackerObserverOutcome::Backoff(backoff);
				}

				let _ = error;

				tracing::warn!(
					issue_id = %lane.issue_id,
					"Skipped Linear execution ledger lookup for a history lane; sensitive tracker details were withheld."
				);

				unavailable = true;
				lane.ledger_outcome = outcome::unavailable_history_ledger_outcome();
			},
		}
	}

	if unavailable { TrackerObserverOutcome::Unavailable } else { TrackerObserverOutcome::Ok }
}

pub(crate) fn hydrate_history_lane_from_ledger_records(
	lane: &mut OperatorHistoryLaneStatus,
	records: &[OperatorHistoryLedgerRecord],
) {
	let Some(record) =
		records.iter().rev().find(|entry| !entry.record.issue_identifier.trim().is_empty())
	else {
		return;
	};
	let metadata = OperatorIssueDisplayMetadata {
		issue_identifier: record.record.issue_identifier.clone(),
		title: None,
		author: None,
		issue_state: None,
		active_label_present: None,
		needs_attention_label_present: None,
	};

	orchestrator::fill_missing_history_lane_issue_metadata(lane, &metadata);
	orchestrator::fill_missing_run_issue_metadata(&mut lane.latest_run, &metadata);

	for attempt in &mut lane.attempts {
		orchestrator::fill_missing_run_issue_metadata(attempt, &metadata);
	}
}
