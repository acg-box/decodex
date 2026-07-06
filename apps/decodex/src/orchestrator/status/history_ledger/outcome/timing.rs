use crate::orchestrator::{
	LinearExecutionEventRecord, OperatorHistoryLedgerRecord, status_run_projection,
};

pub(in crate::orchestrator::status::history_ledger::outcome) fn history_ledger_timing(
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

pub(in crate::orchestrator::status::history_ledger::outcome) fn latest_history_ledger_text<F>(
	records: &[OperatorHistoryLedgerRecord],
	field: F,
) -> Option<String>
where
	F: Fn(&LinearExecutionEventRecord) -> Option<&str>,
{
	records.iter().rev().find_map(|entry| field(&entry.record).map(str::to_owned))
}
