use crate::orchestrator::{LinearExecutionEventRecord, OperatorHistoryLedgerRecord};

pub(in crate::orchestrator::status_history_ledger::outcome) fn history_closeout_status(
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

pub(in crate::orchestrator::status_history_ledger::outcome) fn history_attention_reason(
	final_record: &OperatorHistoryLedgerRecord,
) -> Option<String> {
	match final_record.record.event_type.as_str() {
		"needs_attention" => final_record
			.record
			.summary
			.clone()
			.or_else(|| final_record.record.error_class.clone())
			.or_else(|| final_record.record.next_action.clone()),
		"terminal_failure" => final_record
			.record
			.error_class
			.clone()
			.or_else(|| final_record.record.summary.clone())
			.or_else(|| final_record.record.next_action.clone()),
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
