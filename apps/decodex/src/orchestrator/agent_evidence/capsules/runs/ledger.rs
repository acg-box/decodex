use crate::orchestrator::{
	OperatorHistoryLedgerOutcome, OperatorRunStatus,
	agent_evidence::{AgentEvidenceProjectView, AgentRunLedgerOutcome},
};

pub(in crate::orchestrator::agent_evidence::capsules::runs) fn ledger_outcome_for_run(
	run: &OperatorRunStatus,
	project_view: &AgentEvidenceProjectView<'_>,
) -> Option<AgentRunLedgerOutcome> {
	project_view
		.history_lanes
		.iter()
		.find(|lane| lane.attempts.iter().any(|attempt| attempt.run_id == run.run_id))
		.map(|lane| agent_run_ledger_outcome(&lane.ledger_outcome))
}

pub(in crate::orchestrator::agent_evidence::capsules::runs) fn agent_run_ledger_outcome(
	outcome: &OperatorHistoryLedgerOutcome,
) -> AgentRunLedgerOutcome {
	AgentRunLedgerOutcome {
		ledger_status: outcome.ledger_status.clone(),
		final_outcome: outcome.final_outcome.clone(),
		final_event_type: outcome.final_event_type.clone(),
		final_event_at: outcome.final_event_at.clone(),
		summary: outcome.summary.clone(),
		pr_url: outcome.pr_url.clone(),
		commit_sha: outcome.commit_sha.clone(),
		closeout_status: outcome.closeout_status.clone(),
		needs_attention_reason: outcome.needs_attention_reason.clone(),
		record_count: outcome.record_count,
	}
}
