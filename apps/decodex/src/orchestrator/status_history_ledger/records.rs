use std::{cmp::Ordering, collections::HashSet};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	orchestrator::{LinearExecutionEventRecord, OperatorHistoryLedgerRecord},
	tracker::{TrackerComment, records},
};

pub(crate) fn local_history_ledger_records(
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

pub(crate) fn collect_history_ledger_records(
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

pub(crate) fn compare_history_ledger_record_position(
	left: &OperatorHistoryLedgerRecord,
	right: &OperatorHistoryLedgerRecord,
) -> Ordering {
	left.sort_unix_epoch
		.cmp(&right.sort_unix_epoch)
		.then_with(|| left.comment_index.cmp(&right.comment_index))
}

pub(crate) fn parse_rfc3339_unix_epoch(value: &str) -> Option<i64> {
	OffsetDateTime::parse(value, &Rfc3339).ok().map(|timestamp| timestamp.unix_timestamp())
}
