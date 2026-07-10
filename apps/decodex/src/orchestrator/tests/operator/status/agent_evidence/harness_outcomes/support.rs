use crate::{
	orchestrator::{
		self, PrivateEvidenceReadback,
		tests::operator::status::{
			AuthorityBoundaryChangedSurface, AuthorityBoundaryCheckInput,
			AuthorityBoundaryDisposition, AuthorityBoundaryPolicyDecision,
			AuthorityBoundarySurface, StateStore, TEST_SERVICE_ID, VALIDATION_EVIDENCE_EVENT_TYPE,
			Value,
		},
	},
	state::{PROGRESS_CHECKPOINT_EVENT_TYPE, PROGRESS_CHECKPOINT_SCHEMA},
};

pub(in crate::orchestrator::tests) fn assert_harness_outcome_payload(payload: &Value) {
	assert_eq!(payload["schema"], "decodex.harness_outcome/1");
	assert_eq!(payload["validation"]["result"], "failed");
	assert_eq!(payload["validation"]["failure_count"], 2);
	assert_eq!(
		payload["validation"]["failure_classes"],
		serde_json::json!(["phase_goal_validation_fail", "repo_gate_verify_failed"])
	);
	assert_eq!(payload["repair"]["repair_attempt_observed"], true);
	assert_eq!(payload["review"]["accepted_finding_count"], 1);
	assert_eq!(payload["authority_boundary"]["failed_check_count"], 1);
	assert_eq!(payload["authority_boundary"]["improvement_signal_count"], 1);

	assert_harness_payload_candidate(payload, "authority_underspecified");
	assert_harness_payload_candidate(payload, "architecture_recovery_exhausted");
}

pub(in crate::orchestrator::tests) fn assert_harness_payload_candidate(
	payload: &Value,
	reason_code: &str,
) {
	assert!(
		payload["improvement_candidates"]
			.as_array()
			.expect("candidates should be an array")
			.iter()
			.any(|candidate| candidate["reason_code"] == reason_code)
	);
}

pub(in crate::orchestrator::tests) fn assert_harness_private_readback(
	readback: &PrivateEvidenceReadback,
) {
	assert!(
		readback
			.improvement_candidates
			.iter()
			.any(|candidate| candidate.reason_code == "authority_underspecified")
	);
	assert!(
		readback
			.improvement_candidates
			.iter()
			.any(|candidate| candidate.reason_code == "architecture_recovery_exhausted")
	);
	assert!(readback.architecture_recoveries.iter().any(|recovery| {
		recovery.reason_code == "architecture_recovery_exhausted"
			&& recovery.guardrail_reason.as_deref() == Some("validation_repeat")
			&& recovery.boundary_disposition.as_deref() == Some("within_authority")
			&& recovery.recovery_budget_attempt == Some(2)
			&& recovery.recovery_budget_max_attempts == Some(1)
	}));
	assert!(readback.review_checkpoints.iter().any(|checkpoint| {
		checkpoint.phase == "handoff"
			&& checkpoint.status == "findings"
			&& checkpoint.head_sha.as_deref() == Some("abc123")
			&& checkpoint.round == Some(1)
			&& checkpoint.review_class.as_deref() == Some("full_current_head_review")
			&& checkpoint.risk_class.as_deref() == Some("localized")
			&& checkpoint.compact_eligible == Some(false)
			&& checkpoint.fallback_reason.as_deref() == Some("accepted_findings_present")
			&& checkpoint.accepted_finding_count == 1
			&& checkpoint
				.route_counts
				.iter()
				.any(|count| count.route == "current_blocker" && count.count == 1)
			&& checkpoint.route_next_action.as_deref() == Some("Repair the accepted finding.")
	}));
	assert!(readback.validation_evidence.iter().any(|check| {
		check.phase == "implement_to_validation_ready"
			&& check.decision == "fail"
			&& check.reason_code == "no_effective_delta"
			&& !check.effective_delta_present
	}));
	assert!(readback.boundary_checks.iter().any(|boundary| {
		boundary.disposition == "requires_human"
			&& boundary.attempted_recovery_reason.as_deref() == Some("uncovered_direction")
			&& boundary.changed_surface_count == 1
			&& boundary.improvement_signal_count == 1
	}));

	let rendered = orchestrator::render_private_evidence_readback(readback);

	assert!(rendered.contains("Review Checkpoints"));
	assert!(rendered.contains("review_class: full_current_head_review"));
	assert!(rendered.contains("compact_eligible: false"));
	assert!(rendered.contains("review_fallback_reason: accepted_findings_present"));
	assert!(rendered.contains("route_counts: current_blocker=1"));
	assert!(rendered.contains("route_next_action: Repair the accepted finding."));
	assert!(rendered.contains("Validation Evidences"));
	assert!(rendered.contains("Architecture Recoveries"));
	assert!(rendered.contains("Boundary Checks"));
	assert!(rendered.contains("status: findings"));
	assert!(rendered.contains("reason_code: no_effective_delta"));
	assert!(rendered.contains("disposition: requires_human"));
	assert!(readback.events.iter().all(|event| event.payload.is_none()));
}

pub(in crate::orchestrator::tests) fn record_harness_signal_fixture_events(
	state_store: &StateStore,
) {
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-harness",
			"run-harness",
			2,
			"phase_goal_completed",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "repair_validation_failures",
				"payload": {
					"signal": "validation_fail",
					"status": "complete"
				}
			}),
		)
		.expect("phase goal evidence should append");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-harness",
			"run-harness",
			2,
			"review_checkpoint",
			serde_json::json!({
				"phase": "handoff",
				"status": "findings",
				"head_sha": "abc123",
				"nonclean_rounds": 1,
				"route_counts": [{"route": "current_blocker", "count": 1}],
				"route_next_action": "Repair the accepted finding.",
				"review_class": "full_current_head_review",
				"risk_class": "localized",
				"compact_eligible": false,
				"review_fallback_reason": "accepted_findings_present",
				"review": {
					"review_cost_control": {
						"review_class": "full_current_head_review",
						"risk_class": "localized",
						"compact_eligible": false,
						"fallback_reason": "accepted_findings_present"
					},
					"accepted_findings": [{"summary": "cover the missing edge case"}],
					"rejected_findings": [],
					"finding_route_summary": {
						"route_counts": [{"route": "current_blocker", "count": 1}],
						"next_action": "Repair the accepted finding."
					}
				}
			}),
		)
		.expect("review evidence should append");

	record_harness_progress_checkpoint(state_store);
	record_harness_validation_evidence_event(state_store);

	orchestrator::record_authority_boundary_check_private_event(
		state_store,
		AuthorityBoundaryCheckInput {
			project_id: TEST_SERVICE_ID,
			issue_id: "issue-harness",
			issue_identifier: "PUB-110",
			run_id: "run-harness",
			attempt_number: 2,
			decision_contract_ids: vec!["contract-harness"],
			attempted_recovery_reason: "uncovered_direction",
			changed_surfaces: vec![AuthorityBoundaryChangedSurface {
				surface: AuthorityBoundarySurface::Objective,
				change_summary: "Public behavior would change.",
				policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
				legacy_disposition: AuthorityBoundaryDisposition::RequiresHuman,
			}],
			policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			final_disposition_reason: "Accepted behavior needs explicit authority.",
			improvement_signals: vec![orchestrator::AuthorityBoundaryImprovementSignal {
				kind: "underspecified_decision_contract",
				reason_code: "authority_underspecified",
				target: "decision_contract:contract-harness",
				recommendation: "Record accepted-behavior authority before recovery.",
			}],
		},
	)
	.expect("authority boundary evidence should append");

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-harness",
			"run-harness",
			2,
			"architecture_recovery_terminal",
			serde_json::json!({
				"schema": "decodex.architecture_recovery_terminal/1",
				"record_version": 1,
				"reason_code": "architecture_recovery_exhausted",
				"guardrail_reason": "validation_repeat",
				"boundary_disposition": "within_authority",
				"recovery_budget": {
					"attempt": 2,
					"max_attempts": 1,
				},
			}),
		)
		.expect("architecture recovery evidence should append");
}
pub(in crate::orchestrator::tests) fn record_harness_validation_evidence_event(
	state_store: &StateStore,
) {
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-harness",
			"run-harness",
			2,
			VALIDATION_EVIDENCE_EVENT_TYPE,
			serde_json::json!({
				"schema": "decodex.validation_evidence/2",
				"record_version": 2,
				"phase": "implement_to_validation_ready",
				"decision": "fail",
				"reason_code": "no_effective_delta",
				"objective_coverage": { "covered": true },
				"effective_delta": {
					"present": false,
					"changed_surfaces": ["runtime"],
				},
				"non_goal_check": {
					"passed": true,
					"blocker_count": 0,
				},
				"validation_evidence": {
					"repo_gate_passed": true,
				},
				"next_action": "produce an issue-scoped effective delta before completing the phase goal again",
			}),
		)
		.expect("validation evidence should append");
}

fn record_harness_progress_checkpoint(state_store: &StateStore) {
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-harness",
			"run-harness",
			2,
			PROGRESS_CHECKPOINT_EVENT_TYPE,
			serde_json::json!({
				"schema": PROGRESS_CHECKPOINT_SCHEMA,
				"record_version": 2,
				"phase": "review_repair",
				"focus": "repair accepted finding"
			}),
		)
		.expect("progress evidence should append");
}
