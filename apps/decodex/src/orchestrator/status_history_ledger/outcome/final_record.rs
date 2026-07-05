use crate::orchestrator::{OperatorHistoryLedgerRecord, status_history_ledger::records};

pub(in crate::orchestrator::status_history_ledger::outcome) fn final_history_ledger_record(
	records: &[OperatorHistoryLedgerRecord],
) -> Option<&OperatorHistoryLedgerRecord> {
	records
		.iter()
		.filter(|entry| history_ledger_event_outcome_rank(&entry.record.event_type) > 1)
		.max_by(|left, right| records::compare_history_ledger_record_position(left, right))
		.or_else(|| {
			records
				.iter()
				.max_by(|left, right| records::compare_history_ledger_record_position(left, right))
		})
}

pub(in crate::orchestrator::status_history_ledger::outcome) fn history_ledger_event_outcome_rank(
	event_type: &str,
) -> u8 {
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
