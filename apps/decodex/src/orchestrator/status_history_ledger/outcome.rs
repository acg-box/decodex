use crate::orchestrator::{
	LinearExecutionEventRecord, OperatorHistoryLedgerOutcome, OperatorHistoryLedgerRecord,
	status_history_ledger::records, status_run_projection,
};

pub(crate) fn operator_history_ledger_outcome(
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

pub(crate) fn not_loaded_history_ledger_outcome() -> OperatorHistoryLedgerOutcome {
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

pub(crate) fn missing_history_ledger_outcome() -> OperatorHistoryLedgerOutcome {
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

pub(crate) fn unavailable_history_ledger_outcome() -> OperatorHistoryLedgerOutcome {
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

pub(crate) fn stale_terminal_local_history_ledger_outcome() -> OperatorHistoryLedgerOutcome {
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

fn final_history_ledger_record(
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
