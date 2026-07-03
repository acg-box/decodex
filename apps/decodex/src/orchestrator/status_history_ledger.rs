use std::{cmp::Ordering, collections::HashSet, time::Instant};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	config::ServiceConfig,
	orchestrator::{
		self, LinearExecutionEventRecord, OperatorHistoryLaneStatus, OperatorHistoryLedgerOutcome,
		OperatorHistoryLedgerRecord, OperatorIssueDisplayMetadata, OperatorStatusSnapshot,
		TrackerObserverOutcome, status_run_projection,
	},
	tracker::{IssueTracker, TrackerComment, records},
};

pub(super) fn hydrate_history_lanes_from_linear_ledger<T>(
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
			lane.ledger_outcome = stale_terminal_local_history_ledger_outcome();

			continue;
		}

		match tracker.list_comments(&lane.issue_id) {
			Ok(comments) => {
				let records =
					collect_history_ledger_records(project.service_id(), &lane.issue_id, &comments);

				hydrate_history_lane_from_ledger_records(lane, &records);

				lane.ledger_outcome = operator_history_ledger_outcome(&records);
			},
			Err(error) => {
				if let Some(backoff) = orchestrator::tracker_connector_backoff(
					&error,
					Instant::now(),
					"execution_ledger_status",
				) {
					lane.ledger_outcome = unavailable_history_ledger_outcome();

					return TrackerObserverOutcome::Backoff(backoff);
				}

				let _ = error;

				tracing::warn!(
					issue_id = %lane.issue_id,
					"Skipped Linear execution ledger lookup for a history lane; sensitive tracker details were withheld."
				);

				unavailable = true;
				lane.ledger_outcome = unavailable_history_ledger_outcome();
			},
		}
	}

	if unavailable { TrackerObserverOutcome::Unavailable } else { TrackerObserverOutcome::Ok }
}

pub(super) fn hydrate_history_lane_from_ledger_records(
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

pub(super) fn local_history_ledger_records(
	records: Vec<LinearExecutionEventRecord>,
) -> Vec<OperatorHistoryLedgerRecord> {
	let mut records = records
		.into_iter()
		.enumerate()
		.map(|(comment_index, record)| {
			let event_unix_epoch = parse_rfc3339_unix_epoch(&record.event_timestamp);

			OperatorHistoryLedgerRecord {
				record,
				event_unix_epoch,
				sort_unix_epoch: event_unix_epoch,
				comment_index,
			}
		})
		.collect::<Vec<_>>();

	records.sort_by(compare_history_ledger_record_position);

	records
}

pub(super) fn operator_history_ledger_outcome(
	records: &[OperatorHistoryLedgerRecord],
) -> OperatorHistoryLedgerOutcome {
	let Some(final_record) = final_history_ledger_record(records) else {
		return missing_history_ledger_outcome();
	};
	let ledger_status = if history_ledger_event_outcome_rank(&final_record.record.event_type) > 1 {
		String::from("present")
	} else {
		String::from("partial")
	};
	let (started_at, finished_at, elapsed_seconds) = history_ledger_timing(records);

	OperatorHistoryLedgerOutcome {
		ledger_status,
		final_outcome: final_record.record.event_type.clone(),
		final_event_type: Some(final_record.record.event_type.clone()),
		final_event_at: Some(final_record.record.event_timestamp.clone()),
		summary: history_ledger_summary(final_record, records),
		pr_url: latest_history_ledger_text(records, |record| record.pr_url.as_deref()),
		commit_sha: latest_history_ledger_text(records, |record| record.commit_sha.as_deref()),
		branch: latest_history_ledger_text(records, |record| record.branch.as_deref()),
		closeout_status: history_closeout_status(final_record, records),
		needs_attention_reason: history_attention_reason(final_record),
		lifecycle_started_at: started_at,
		lifecycle_finished_at: finished_at,
		lifecycle_elapsed_seconds: elapsed_seconds,
		record_count: records.len(),
	}
}

pub(super) fn collect_history_ledger_records(
	service_id: &str,
	issue_id: &str,
	comments: &[TrackerComment],
) -> Vec<OperatorHistoryLedgerRecord> {
	let mut seen_keys = HashSet::new();
	let mut records = comments
		.iter()
		.enumerate()
		.filter_map(|(comment_index, comment)| {
			let record = records::parse_linear_execution_event_record(&comment.body)?;

			if record.service_id != service_id || record.issue_id != issue_id {
				return None;
			}
			if !seen_keys.insert(record.idempotency_key.clone()) {
				return None;
			}

			let event_unix_epoch = parse_rfc3339_unix_epoch(&record.event_timestamp);
			let comment_unix_epoch = parse_rfc3339_unix_epoch(&comment.created_at);

			Some(OperatorHistoryLedgerRecord {
				record,
				event_unix_epoch,
				sort_unix_epoch: event_unix_epoch.or(comment_unix_epoch),
				comment_index,
			})
		})
		.collect::<Vec<_>>();

	records.sort_by(compare_history_ledger_record_position);

	records
}

pub(super) fn compare_history_ledger_record_position(
	left: &OperatorHistoryLedgerRecord,
	right: &OperatorHistoryLedgerRecord,
) -> Ordering {
	left.sort_unix_epoch
		.cmp(&right.sort_unix_epoch)
		.then_with(|| left.comment_index.cmp(&right.comment_index))
}

pub(super) fn parse_rfc3339_unix_epoch(value: &str) -> Option<i64> {
	OffsetDateTime::parse(value, &Rfc3339).ok().map(|timestamp| timestamp.unix_timestamp())
}

pub(super) fn not_loaded_history_ledger_outcome() -> OperatorHistoryLedgerOutcome {
	OperatorHistoryLedgerOutcome {
		ledger_status: String::from("not_loaded"),
		final_outcome: String::from("local_attempt_history"),
		final_event_type: None,
		final_event_at: None,
		summary: Some(String::from(
			"Linear execution ledger was not loaded for this local-only snapshot.",
		)),
		pr_url: None,
		commit_sha: None,
		branch: None,
		closeout_status: None,
		needs_attention_reason: None,
		lifecycle_started_at: None,
		lifecycle_finished_at: None,
		lifecycle_elapsed_seconds: None,
		record_count: 0,
	}
}

pub(super) fn missing_history_ledger_outcome() -> OperatorHistoryLedgerOutcome {
	OperatorHistoryLedgerOutcome {
		ledger_status: String::from("missing"),
		final_outcome: String::from("execution_ledger_missing"),
		final_event_type: None,
		final_event_at: None,
		summary: Some(String::from(
			"No decodex.linear_execution_event records are available for this history lane.",
		)),
		pr_url: None,
		commit_sha: None,
		branch: None,
		closeout_status: None,
		needs_attention_reason: None,
		lifecycle_started_at: None,
		lifecycle_finished_at: None,
		lifecycle_elapsed_seconds: None,
		record_count: 0,
	}
}

fn final_history_ledger_record(
	records: &[OperatorHistoryLedgerRecord],
) -> Option<&OperatorHistoryLedgerRecord> {
	records
		.iter()
		.filter(|entry| history_ledger_event_outcome_rank(&entry.record.event_type) > 1)
		.max_by(|left, right| compare_history_ledger_record_position(left, right))
		.or_else(|| {
			records.iter().max_by(|left, right| compare_history_ledger_record_position(left, right))
		})
}

fn history_ledger_event_outcome_rank(event_type: &str) -> u8 {
	match event_type {
		"cleanup_complete" => 7,
		"closeout" => 6,
		"needs_attention" | "terminal_failure" => 5,
		"landed" => 4,
		"review_handoff" | "repair_handoff" => 3,
		"pr_opened" | "pr_updated" => 2,
		_ => 1,
	}
}

fn history_ledger_timing(
	records: &[OperatorHistoryLedgerRecord],
) -> (Option<String>, Option<String>, Option<i64>) {
	let started = records.iter().filter_map(|entry| entry.event_unix_epoch).min();
	let finished = records.iter().filter_map(|entry| entry.event_unix_epoch).max();
	let elapsed = started
		.zip(finished)
		.and_then(|(started, finished)| finished.checked_sub(started))
		.filter(|elapsed| *elapsed >= 0);

	(
		started.and_then(|timestamp| {
			status_run_projection::format_optional_unix_timestamp(Some(timestamp))
		}),
		finished.and_then(|timestamp| {
			status_run_projection::format_optional_unix_timestamp(Some(timestamp))
		}),
		elapsed,
	)
}

fn history_ledger_summary(
	final_record: &OperatorHistoryLedgerRecord,
	records: &[OperatorHistoryLedgerRecord],
) -> Option<String> {
	if history_ledger_event_outcome_rank(&final_record.record.event_type) > 1 {
		return final_record.record.summary.clone();
	}

	Some(format!(
		"Ledger has {} records but no final lane outcome yet; latest event is `{}`.",
		records.len(),
		final_record.record.event_type
	))
}

fn latest_history_ledger_text<F>(
	records: &[OperatorHistoryLedgerRecord],
	field: F,
) -> Option<String>
where
	F: Fn(&LinearExecutionEventRecord) -> Option<&str>,
{
	records.iter().rev().find_map(|entry| field(&entry.record).map(str::to_owned))
}

fn history_closeout_status(
	final_record: &OperatorHistoryLedgerRecord,
	records: &[OperatorHistoryLedgerRecord],
) -> Option<String> {
	match final_record.record.event_type.as_str() {
		"closeout" => closeout_status_from_record(&final_record.record),
		"cleanup_complete" => final_record.record.cleanup_status.clone().or_else(|| {
			records.iter().rev().find_map(|entry| {
				(entry.record.event_type == "closeout")
					.then(|| closeout_status_from_record(&entry.record))
					.flatten()
			})
		}),
		_ => None,
	}
}

fn closeout_status_from_record(record: &LinearExecutionEventRecord) -> Option<String> {
	record
		.target_state
		.clone()
		.or_else(|| record.validation_result.clone())
		.or_else(|| Some(String::from("recorded")))
}

fn history_attention_reason(final_record: &OperatorHistoryLedgerRecord) -> Option<String> {
	match final_record.record.event_type.as_str() {
		"needs_attention" | "terminal_failure" => final_record
			.record
			.summary
			.clone()
			.or_else(|| final_record.record.error_class.clone())
			.or_else(|| final_record.record.next_action.clone()),
		_ => None,
	}
}

fn unavailable_history_ledger_outcome() -> OperatorHistoryLedgerOutcome {
	OperatorHistoryLedgerOutcome {
		ledger_status: String::from("unavailable"),
		final_outcome: String::from("ledger_unavailable"),
		final_event_type: None,
		final_event_at: None,
		summary: Some(String::from(
			"Linear execution ledger records could not be loaded for this issue.",
		)),
		pr_url: None,
		commit_sha: None,
		branch: None,
		closeout_status: None,
		needs_attention_reason: None,
		lifecycle_started_at: None,
		lifecycle_finished_at: None,
		lifecycle_elapsed_seconds: None,
		record_count: 0,
	}
}

fn stale_terminal_local_history_ledger_outcome() -> OperatorHistoryLedgerOutcome {
	OperatorHistoryLedgerOutcome {
		ledger_status: String::from("local_terminal_residue"),
		final_outcome: String::from("local_terminal_residue"),
		final_event_type: None,
		final_event_at: None,
		summary: Some(String::from(
			"Linear ledger lookup skipped for terminal unleased local residue with an identifier-style issue id.",
		)),
		pr_url: None,
		commit_sha: None,
		branch: None,
		closeout_status: None,
		needs_attention_reason: None,
		lifecycle_started_at: None,
		lifecycle_finished_at: None,
		lifecycle_elapsed_seconds: None,
		record_count: 0,
	}
}
