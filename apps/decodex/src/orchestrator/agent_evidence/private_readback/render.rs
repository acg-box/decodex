use crate::orchestrator::agent_evidence::{
	PrivateEvidenceArchitectureRecoverySummary, PrivateEvidenceBoundaryCheckSummary,
	PrivateEvidenceDecisionRequestSummary, PrivateEvidencePayloadSummary,
	PrivateEvidencePhaseAcceptanceSummary, PrivateEvidenceReadback, PrivateEvidenceReadbackEvent,
	PrivateEvidenceRepoGateFailureSummary, PrivateEvidenceReviewCheckpointSummary,
};
use crate::orchestrator::harness_improvement::HarnessImprovementCandidateSummary;

pub(in crate::orchestrator) fn render_private_evidence_readback(
	readback: &PrivateEvidenceReadback,
) -> String {
	let mut output = String::new();

	append_private_evidence_readback_header(&mut output, readback);
	append_private_evidence_decision_requests(&mut output, &readback.decision_requests);
	append_private_evidence_review_checkpoints(&mut output, &readback.review_checkpoints);
	append_private_evidence_repo_gate_failures(&mut output, &readback.repo_gate_failures);
	append_private_evidence_phase_acceptance_checks(&mut output, &readback.phase_acceptance_checks);
	append_private_evidence_architecture_recoveries(&mut output, &readback.architecture_recoveries);
	append_private_evidence_boundary_checks(&mut output, &readback.boundary_checks);
	append_private_evidence_improvement_candidates(&mut output, &readback.improvement_candidates);
	append_private_evidence_events(&mut output, &readback.events);

	output
}

pub(in crate::orchestrator) fn render_private_evidence_payload_summary(
	summary: &PrivateEvidencePayloadSummary,
) -> String {
	let keys = if summary.keys.is_empty() { String::from("none") } else { summary.keys.join(",") };
	let preview =
		if summary.preview.is_empty() { String::from("none") } else { summary.preview.join("; ") };
	let redacted = if summary.redacted_default_keys.is_empty() {
		String::from("none")
	} else {
		summary.redacted_default_keys.join(",")
	};

	format!(
		"kind={} bytes={} keys={} preview={} redacted_default_keys={}",
		summary.kind, summary.byte_count, keys, preview, redacted
	)
}

fn append_private_evidence_readback_header(
	output: &mut String,
	readback: &PrivateEvidenceReadback,
) {
	output.push_str(&format!("Project: {}\n", readback.project_id));
	output.push_str("Private Execution Evidence\n");
	output.push_str(&format!("issue_selector: {}\n", readback.issue_selector));
	output.push_str(&format!("issue_id: {}\n", readback.issue_id));
	output.push_str(&format!(
		"issue_identifier: {}\n",
		readback.issue_identifier.as_deref().unwrap_or("none")
	));
	output.push_str(&format!("run_id: {}\n", readback.run_id));
	output.push_str(&format!("attempt: {}\n", readback.attempt_number));
	output.push_str(&format!("source: {}\n", readback.source));
	output.push_str(&format!("evidence_ref: {}\n", readback.evidence_ref));
	output.push_str(&format!("payload_mode: {}\n", readback.payload_mode));
	output.push_str(&format!("event_count: {}\n", readback.event_count));
	output.push_str(&format!(
		"improvement_candidate_count: {}\n",
		readback.improvement_candidates.len()
	));
	output.push_str(&format!("decision_request_count: {}\n", readback.decision_requests.len()));
	output.push_str(&format!("review_checkpoint_count: {}\n", readback.review_checkpoints.len()));
	output.push_str(&format!(
		"architecture_recovery_count: {}\n",
		readback.architecture_recoveries.len()
	));
	output.push_str(&format!("boundary_check_count: {}\n", readback.boundary_checks.len()));
	output.push_str(&format!(
		"latest_event_type: {}\n",
		readback.latest_event_type.as_deref().unwrap_or("none")
	));
	output.push_str(&format!(
		"latest_event_at: {}\n",
		readback.latest_event_at.as_deref().unwrap_or("none")
	));

	if !readback.warnings.is_empty() {
		output.push_str(&format!("warnings: {}\n", readback.warnings.join(", ")));
	}
}

fn append_private_evidence_decision_requests(
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

fn append_private_evidence_review_checkpoints(
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

fn append_private_evidence_repo_gate_failures(
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

fn append_private_evidence_phase_acceptance_checks(
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

fn append_private_evidence_architecture_recoveries(
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

fn append_private_evidence_boundary_checks(
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

fn append_private_evidence_improvement_candidates(
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

fn append_private_evidence_events(output: &mut String, events: &[PrivateEvidenceReadbackEvent]) {
	output.push_str("\nEvents\n");

	if events.is_empty() {
		output.push_str("- none\n");

		return;
	}

	for event in events {
		output.push_str(&format!(
			"- record_id: {}\n  event_type: {}\n  recorded_at: {}\n  payload: {}\n",
			event.record_id,
			event.event_type,
			event.recorded_at,
			render_private_evidence_payload_summary(&event.payload_summary)
		));

		if let Some(payload) = &event.payload {
			output.push_str(&format!("  full_payload: {}\n", payload));
		}
	}
}
