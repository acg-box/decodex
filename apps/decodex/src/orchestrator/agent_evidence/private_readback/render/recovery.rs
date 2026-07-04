use crate::orchestrator::{
	agent_evidence::{
		PrivateEvidenceArchitectureRecoverySummary, PrivateEvidenceBoundaryCheckSummary,
	},
	harness_improvement::HarnessImprovementCandidateSummary,
};

pub(in crate::orchestrator::agent_evidence::private_readback::render) fn append_private_evidence_architecture_recoveries(
	output: &mut String,
	architecture_recoveries: &[PrivateEvidenceArchitectureRecoverySummary],
) {
	output.push_str("\nArchitecture Recoveries\n");

	if architecture_recoveries.is_empty() {
		output.push_str("- none\n");
	} else {
		for recovery in architecture_recoveries {
			output.push_str(&format!(
				"- reason_code: {}\n  guardrail_reason: {}\n  boundary_disposition: {}\n  boundary_policy: {}\n  enhanced_evidence: {}\n  blocks_landing: {}\n  budget: {}/{}\n  next_action: {}\n",
				recovery.reason_code,
				recovery.guardrail_reason.as_deref().unwrap_or("none"),
				recovery.boundary_disposition.as_deref().unwrap_or("none"),
				recovery
					.boundary_policy_decision
					.as_deref()
					.unwrap_or("none"),
				recovery.requires_enhanced_evidence,
				recovery.blocks_landing,
				recovery
					.recovery_budget_attempt
					.map_or_else(|| String::from("none"), |attempt| attempt.to_string()),
				recovery
					.recovery_budget_max_attempts
					.map_or_else(|| String::from("none"), |max_attempts| max_attempts.to_string()),
				recovery.next_action
			));
		}
	}
}

pub(in crate::orchestrator::agent_evidence::private_readback::render) fn append_private_evidence_boundary_checks(
	output: &mut String,
	boundary_checks: &[PrivateEvidenceBoundaryCheckSummary],
) {
	output.push_str("\nBoundary Checks\n");

	if boundary_checks.is_empty() {
		output.push_str("- none\n");
	} else {
		for boundary in boundary_checks {
			output.push_str(&format!(
				"- disposition: {}\n  policy: {}\n  enhanced_evidence: {}\n  blocks_landing: {}\n  reason: {}\n  attempted_recovery: {}\n  decision_contracts: {}\n  changed_surfaces: {}\n  improvement_signals: {}\n  next_action: {}\n",
				boundary.disposition,
				boundary.policy_decision,
				boundary.requires_enhanced_evidence,
				boundary.blocks_landing,
				boundary.reason.as_deref().unwrap_or("none"),
				boundary
					.attempted_recovery_reason
					.as_deref()
					.unwrap_or("none"),
				boundary.decision_contract_count,
				boundary.changed_surface_count,
				boundary.improvement_signal_count,
				boundary.next_action
			));
		}
	}
}

pub(in crate::orchestrator::agent_evidence::private_readback::render) fn append_private_evidence_improvement_candidates(
	output: &mut String,
	improvement_candidates: &[HarnessImprovementCandidateSummary],
) {
	output.push_str("\nImprovement Candidates\n");

	if improvement_candidates.is_empty() {
		output.push_str("- none\n");
	} else {
		for candidate in improvement_candidates {
			output.push_str(&format!(
				"- kind: {}\n  reason_code: {}\n  target: {}\n  source_event_count: {}\n  recommendation: {}\n",
				candidate.kind,
				candidate.reason_code,
				candidate.target,
				candidate.source_event_count,
				candidate.recommendation
			));
		}
	}
}
