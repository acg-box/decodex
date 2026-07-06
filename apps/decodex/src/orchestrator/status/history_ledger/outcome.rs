mod closeout;
mod defaults;
mod final_record;
mod timing;

pub(crate) use self::defaults::{
	missing_history_ledger_outcome, not_loaded_history_ledger_outcome,
	stale_terminal_local_history_ledger_outcome, unavailable_history_ledger_outcome,
};

use crate::orchestrator::{OperatorHistoryLedgerOutcome, OperatorHistoryLedgerRecord};

pub(crate) fn operator_history_ledger_outcome(
	records: &[OperatorHistoryLedgerRecord],
) -> OperatorHistoryLedgerOutcome {
	let Some(final_record) = final_record::final_history_ledger_record(records) else {
		return defaults::missing_history_ledger_outcome();
	};
	let ledger_status =
		if final_record::history_ledger_event_outcome_rank(&final_record.record.event_type) > 1 {
			String::from("present")
		} else {
			String::from("partial")
		};
	let (started_at, finished_at, elapsed_seconds) = timing::history_ledger_timing(records);

	OperatorHistoryLedgerOutcome {
		ledger_status,
		final_outcome: final_record.record.event_type.clone(),
		final_event_type: Some(final_record.record.event_type.clone()),
		final_event_at: Some(final_record.record.event_timestamp.clone()),
		summary: history_ledger_summary(final_record, records),
		pr_url: timing::latest_history_ledger_text(records, |record| record.pr_url.as_deref()),
		commit_sha: timing::latest_history_ledger_text(records, |record| {
			record.commit_sha.as_deref()
		}),
		branch: timing::latest_history_ledger_text(records, |record| record.branch.as_deref()),
		closeout_status: closeout::history_closeout_status(final_record, records),
		needs_attention_reason: closeout::history_attention_reason(final_record),
		lifecycle_started_at: started_at,
		lifecycle_finished_at: finished_at,
		lifecycle_elapsed_seconds: elapsed_seconds,
		record_count: records.len(),
	}
}

fn history_ledger_summary(
	final_record: &OperatorHistoryLedgerRecord,
	records: &[OperatorHistoryLedgerRecord],
) -> Option<String> {
	if final_record::history_ledger_event_outcome_rank(&final_record.record.event_type) > 1 {
		return final_record.record.summary.clone();
	}

	Some(format!(
		"Ledger has {} records but no final lane outcome yet; latest event is `{}`.",
		records.len(),
		final_record.record.event_type
	))
}
