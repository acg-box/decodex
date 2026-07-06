use crate::orchestrator::OperatorHistoryLedgerOutcome;

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
