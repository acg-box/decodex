use crate::orchestrator::agent_evidence::{
	PrivateEvidenceDecisionRequestSummary, PrivateEvidencePhaseAcceptanceSummary,
	PrivateEvidenceRepoGateFailureSummary, PrivateEvidenceReviewCheckpointSummary,
};

pub(in crate::orchestrator::agent_evidence::private_readback::render) fn append_private_evidence_decision_requests(
	output: &mut String,
	decision_requests: &[PrivateEvidenceDecisionRequestSummary],
) {
	output.push_str("\nDecision Requests\n");

	if decision_requests.is_empty() {
		output.push_str("- none\n");
	} else {
		for request in decision_requests {
			output.push_str(&format!(
				"- id: {}\n  phase: {}\n  reason: {}\n  boundary: {}\n  next_action: {}\n",
				request.decision_request_id,
				request.phase,
				request.reason,
				request.boundary,
				request.next_action
			));
		}
	}
}

pub(in crate::orchestrator::agent_evidence::private_readback::render) fn append_private_evidence_review_checkpoints(
	output: &mut String,
	review_checkpoints: &[PrivateEvidenceReviewCheckpointSummary],
) {
	output.push_str("\nReview Checkpoints\n");

	if review_checkpoints.is_empty() {
		output.push_str("- none\n");
	} else {
		for checkpoint in review_checkpoints {
			let active_fingerprints = if checkpoint.active_fingerprints.is_empty() {
				String::from("none")
			} else {
				checkpoint.active_fingerprints.join(", ")
			};
			let route_counts = if checkpoint.route_counts.is_empty() {
				String::from("none")
			} else {
				checkpoint
					.route_counts
					.iter()
					.map(|count| format!("{}={}", count.route, count.count))
					.collect::<Vec<_>>()
					.join(", ")
			};

			output.push_str(&format!(
				"- phase: {}\n  status: {}\n  head_sha: {}\n  round: {}\n  review_class: {}\n  risk_class: {}\n  compact_eligible: {}\n  review_fallback_reason: {}\n  active_fingerprints: {}\n  stop_fingerprint: {}\n  accepted_findings: {}\n  rejected_findings: {}\n  route_counts: {}\n  route_next_action: {}\n  next_action: {}\n",
				checkpoint.phase,
				checkpoint.status,
				checkpoint.head_sha.as_deref().unwrap_or("none"),
				checkpoint
					.round
					.map_or_else(|| String::from("none"), |round| round.to_string()),
				checkpoint.review_class.as_deref().unwrap_or("none"),
				checkpoint.risk_class.as_deref().unwrap_or("none"),
				checkpoint.compact_eligible.map_or("none", |eligible| {
					if eligible {
						"true"
					} else {
						"false"
					}
				}),
				checkpoint.fallback_reason.as_deref().unwrap_or("none"),
				active_fingerprints,
				checkpoint.stop_fingerprint.as_deref().unwrap_or("none"),
				checkpoint.accepted_finding_count,
				checkpoint.rejected_finding_count,
				route_counts,
				checkpoint.route_next_action.as_deref().unwrap_or("none"),
				checkpoint.next_action
			));
		}
	}
}

pub(in crate::orchestrator::agent_evidence::private_readback::render) fn append_private_evidence_repo_gate_failures(
	output: &mut String,
	failures: &[PrivateEvidenceRepoGateFailureSummary],
) {
	if failures.is_empty() {
		return;
	}

	output.push_str("\nRepo Gate Failures\n");

	for failure in failures {
		let problem_lines = if failure.problem_lines.is_empty() {
			String::from("none")
		} else {
			failure.problem_lines.join(" | ")
		};

		output.push_str(&format!(
			"- record_id: {}\n  phase: {}\n  error_class: {}\n  disposition: {}\n  stage: {}\n  failed_command: {}\n  exit_status: {}\n  summary: {}\n  problem_lines: {}\n",
			failure.record_id,
			failure.phase,
			failure.error_class,
			failure.disposition,
			failure.stage.as_deref().unwrap_or("none"),
			failure.failed_command.as_deref().unwrap_or("none"),
			failure
				.exit_status
				.map_or_else(|| String::from("none"), |status| status.to_string()),
			failure.summary.as_deref().unwrap_or("none"),
			problem_lines
		));
	}
}

pub(in crate::orchestrator::agent_evidence::private_readback::render) fn append_private_evidence_phase_acceptance_checks(
	output: &mut String,
	checks: &[PrivateEvidencePhaseAcceptanceSummary],
) {
	if checks.is_empty() {
		return;
	}

	output.push_str("Phase Acceptance Checks\n");

	for check in checks {
		let surfaces = if check.changed_surfaces.is_empty() {
			String::from("none")
		} else {
			check.changed_surfaces.join(",")
		};

		output.push_str(&format!(
			"- phase: {}\n  decision: {}\n  reason_code: {}\n  objective_covered: {}\n  effective_delta: {}\n  changed_surfaces: {}\n  non_goal_passed: {}\n  validation_passed: {}\n  next_action: {}\n",
			check.phase,
			check.decision,
			check.reason_code,
			check.objective_covered,
			check.effective_delta_present,
			surfaces,
			check.non_goal_passed,
			check.validation_passed,
			check.next_action
		));
	}
}
