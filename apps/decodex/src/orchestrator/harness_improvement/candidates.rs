use super::{
	HarnessImprovementCandidateSummary, HarnessLinearProjectionSummary, HarnessOutcomeContract,
	HarnessOutcomeProgram, HarnessOutcomeRecordInput, HarnessOutcomeSignals, Value,
};

pub(super) fn harness_improvement_candidates(
	input: &HarnessOutcomeRecordInput<'_>,
	contracts: &[HarnessOutcomeContract],
	programs: &[HarnessOutcomeProgram],
	signals: &HarnessOutcomeSignals,
	linear_projection: &HarnessLinearProjectionSummary,
) -> Vec<HarnessImprovementCandidateSummary> {
	let mut candidates = std::collections::BTreeMap::new();

	push_contract_candidates(&mut candidates, input, contracts, programs);
	push_signal_candidates(&mut candidates, input, signals, linear_projection);

	candidates.into_values().collect()
}

fn push_contract_candidates(
	candidates: &mut std::collections::BTreeMap<String, HarnessImprovementCandidateSummary>,
	input: &HarnessOutcomeRecordInput<'_>,
	contracts: &[HarnessOutcomeContract],
	programs: &[HarnessOutcomeProgram],
) {
	if contracts.is_empty() {
		insert_candidate(
			candidates,
			"missing_issue_template_field",
			"contract_provenance_missing",
			&format!("issue:{}", input.issue_identifier),
			0,
			"Add source intent and Decision Contract id/provenance to generated issue briefs.",
		);

		return;
	}

	for contract in contracts {
		if contract.missing_decision_count > 0 {
			insert_candidate(
				candidates,
				"underspecified_decision_contract",
				"missing_decisions",
				&format!("decision_contract:{}", contract.contract_id),
				0,
				"Require missing decisions to be resolved before promotion or queueing.",
			);
		}
		if contract.generated_issue_ids.is_empty()
			&& contract.generated_issue_identifiers.is_empty()
		{
			insert_candidate(
				candidates,
				"missing_issue_template_field",
				"generated_issue_links_missing",
				&format!("decision_contract:{}", contract.contract_id),
				0,
				"Record generated issue ids or identifiers when research is promoted.",
			);
		}
		if contract.conflict_domains.is_empty() {
			insert_candidate(
				candidates,
				"missing_issue_template_field",
				"conflict_domains_missing",
				&format!("decision_contract:{}", contract.contract_id),
				0,
				"Require conflict-domain notes in contracts or generated issue templates.",
			);
		}
	}
	for program in programs.iter().filter(|program| program.node_count == 0) {
		insert_candidate(
			candidates,
			"stale_readiness_model",
			"execution_program_has_no_nodes",
			&format!("execution_program:{}", program.program_id),
			0,
			"Regenerate internal Execution Program readiness from the accepted contract.",
		);
	}
}

pub(super) fn push_signal_candidates(
	candidates: &mut std::collections::BTreeMap<String, HarnessImprovementCandidateSummary>,
	input: &HarnessOutcomeRecordInput<'_>,
	signals: &HarnessOutcomeSignals,
	linear_projection: &HarnessLinearProjectionSummary,
) {
	if signals.validation_failure_count > 0 {
		insert_candidate(
			candidates,
			"weak_prompt",
			"validation_failed_after_generation",
			&format!("issue:{}", input.issue_identifier),
			signals.validation_failure_count,
			"Tighten phase prompts or preflight checks around the failing validation class.",
		);
	}
	if signals.accepted_finding_count > 0 {
		insert_candidate(
			candidates,
			"weak_prompt",
			"accepted_review_findings",
			&format!("issue:{}", input.issue_identifier),
			signals.accepted_finding_count,
			"Convert accepted reviewer findings into prompt, skill, or validator hardening.",
		);
	}
	if signals.accepted_finding_count > 0 && signals.guardrail_reasons.contains("no_effective_diff")
	{
		insert_candidate(
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

		insert_candidate(
			candidates,
			kind,
			reason,
			&format!("issue:{}", input.issue_identifier),
			signals.guardrail_reasons.len(),
			recommendation,
		);
	}
	for candidate in &signals.authority_boundary_candidates {
		insert_candidate(
			candidates,
			&candidate.kind,
			&candidate.reason_code,
			&candidate.target,
			candidate.source_event_count,
			&candidate.recommendation,
		);
	}

	if signals.architecture_recovery_budget_exhausted_count > 0 {
		insert_candidate(
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
		insert_candidate(
			candidates,
			"underspecified_decision_contract",
			"uncovered_direction",
			&format!("issue:{}", input.issue_identifier),
			1,
			"Feed the uncovered direction back into the Decision Contract before retrying.",
		);
	}
}

pub(super) fn authority_boundary_final_reason_mentions_underspecified(payload: &Value) -> bool {
	let reason = payload
		.get("final_disposition")
		.and_then(|value| json_string(value.get("reason")))
		.or_else(|| json_string(payload.get("final_disposition_reason")));

	reason.is_some_and(|reason| {
		let reason = reason.to_ascii_lowercase();

		reason.contains("underspecified")
			|| reason.contains("missing contract")
			|| reason.contains("missing authority")
	})
}

pub(super) fn first_decision_contract_target(payload: &Value) -> Option<String> {
	payload
		.get("decision_contract_ids")
		.and_then(Value::as_array)?
		.iter()
		.filter_map(Value::as_str)
		.find(|contract_id| !contract_id.is_empty())
		.map(|contract_id| format!("decision_contract:{contract_id}"))
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
		_ =>
			("weak_prompt", "Tighten loop instructions so future attempts stop or repair earlier."),
	}
}

fn insert_candidate(
	candidates: &mut std::collections::BTreeMap<String, HarnessImprovementCandidateSummary>,
	kind: &str,
	reason_code: &str,
	target: &str,
	source_event_count: usize,
	recommendation: &str,
) {
	let key = format!("{kind}:{reason_code}:{target}");

	candidates.entry(key).or_insert_with(|| HarnessImprovementCandidateSummary {
		kind: kind.to_owned(),
		reason_code: reason_code.to_owned(),
		target: target.to_owned(),
		source_event_count,
		recommendation: recommendation.to_owned(),
	});
}

pub(super) fn harness_candidates_from_payload(
	payload: &Value,
) -> Vec<HarnessImprovementCandidateSummary> {
	payload
		.get("improvement_candidates")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(|candidate| {
			Some(HarnessImprovementCandidateSummary {
				kind: json_string(candidate.get("kind"))?,
				reason_code: json_string(candidate.get("reason_code"))?,
				target: json_string(candidate.get("target"))?,
				source_event_count: candidate
					.get("source_event_count")
					.and_then(Value::as_u64)
					.and_then(|value| usize::try_from(value).ok())
					.unwrap_or(0),
				recommendation: json_string(candidate.get("recommendation"))?,
			})
		})
		.collect()
}

pub(super) fn json_string(value: Option<&Value>) -> Option<String> {
	value.and_then(Value::as_str).filter(|value| !value.is_empty()).map(str::to_owned)
}

pub(super) fn json_array_len(value: Option<&Value>) -> usize {
	value.and_then(Value::as_array).map_or(0, Vec::len)
}
