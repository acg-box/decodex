use crate::orchestrator::{OperatorHistoryLedgerRecord, status_history_ledger::records};

pub(in crate::orchestrator::status::history_ledger::outcome) fn final_history_ledger_record(
	records: &[OperatorHistoryLedgerRecord],
) -> Option<&OperatorHistoryLedgerRecord> {
	let latest = records
		.iter()
		.max_by(|left, right| records::compare_history_ledger_record_position(left, right))?;

	(history_ledger_event_outcome_rank(&latest.record.event_type) > 1).then_some(latest)
}

pub(in crate::orchestrator::status::history_ledger::outcome) fn history_ledger_event_outcome_rank(
	event_type: &str,
) -> u8 {
	match event_type {
		"needs_attention" | "terminal_failure" => 5,
		_ => 1,
	}
}
