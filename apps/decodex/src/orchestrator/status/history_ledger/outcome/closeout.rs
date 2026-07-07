use crate::orchestrator::OperatorHistoryLedgerRecord;

pub(in crate::orchestrator::status::history_ledger::outcome) fn history_attention_reason(
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
