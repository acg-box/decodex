use std::collections::BTreeMap;

use crate::orchestrator::harness_improvement::{
	HarnessImprovementCandidateSummary, HarnessLinearProjectionSummary, HarnessOutcomeRecordInput,
	HarnessOutcomeSignals, candidates::util,
};

pub(in crate::orchestrator::harness_improvement) fn push_signal_candidates(
	candidates: &mut BTreeMap<String, HarnessImprovementCandidateSummary>,
	input: &HarnessOutcomeRecordInput<'_>,
	signals: &HarnessOutcomeSignals,
	linear_projection: &HarnessLinearProjectionSummary,
) {
	if signals.accepted_finding_count > 0 && signals.guardrail_reasons.contains("no_effective_diff")
	{
		util::insert_candidate(
			candidates,
			"state_machine_gap",
			"review_repair_no_effective_diff_after_findings",
			&format!("issue:{}", input.issue_identifier),
			signals.accepted_finding_count,
			"Runtime should either record a fresh clean review checkpoint, continue review repair with a concrete diff, or stop with needs-attention instead of generic retryable failure.",
		);
	}

	for reason in &signals.guardrail_reasons {
		let (kind, recommendation) = guardrail_candidate_kind(reason);

		util::insert_candidate(
			candidates,
			kind,
			reason,
			&format!("issue:{}", input.issue_identifier),
			signals.guardrail_reasons.len(),
			recommendation,
		);
	}
	for candidate in &signals.authority_boundary_candidates {
		util::insert_candidate(
			candidates,
			&candidate.kind,
			&candidate.reason_code,
			&candidate.target,
			candidate.source_event_count,
			&candidate.recommendation,
		);
	}

	if signals.architecture_recovery_budget_exhausted_count > 0 {
		util::insert_candidate(
			candidates,
			"recovery_budget_exhausted",
			"architecture_recovery_exhausted",
			&format!("issue:{}", input.issue_identifier),
			signals.architecture_recovery_budget_exhausted_count,
			"Increase recovery evidence quality or require a new accepted architecture decision before retrying.",
		);
	}
	if input.error_class == Some("uncovered_direction")
		|| linear_projection.final_error_class.as_deref() == Some("uncovered_direction")
	{
		util::insert_candidate(
			candidates,
			"underspecified_decision_contract",
			"uncovered_direction",
			&format!("issue:{}", input.issue_identifier),
			1,
			"Feed the uncovered direction back into the Decision Contract before retrying.",
		);
	}
}

fn guardrail_candidate_kind(reason: &str) -> (&'static str, &'static str) {
	match reason {
		"dependency_program_stale" => (
			"stale_readiness_model",
			"Refresh dependency readiness and queue-label policy for the affected program node.",
		),
		"uncovered_direction" => (
			"underspecified_decision_contract",
			"Feed the uncovered direction back into the Decision Contract before retrying.",
		),
		"validation_repeat" | "remaining_delta_unchanged" => (
			"missing_validator",
			"Promote the repeated failure into an earlier deterministic validator or fixture.",
		),
		_ => (
			"harness_contract_gap",
			"Record a durable harness gap only after the repeated failure class has concrete evidence.",
		),
	}
}
