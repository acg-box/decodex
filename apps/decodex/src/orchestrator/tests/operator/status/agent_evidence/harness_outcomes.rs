use orchestrator::{HarnessOutcomeKind, HarnessOutcomeRecordInput, PrivateEvidenceReadback};

use crate::orchestrator::tests::operator::status::{
	self, AuthorityBoundaryChangedSurface, AuthorityBoundaryCheckInput,
	AuthorityBoundaryDisposition, AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface,
	DecisionContract, EvidenceRequest, PHASE_ACCEPTANCE_CHECK_EVENT_TYPE, StateStore,
	TEST_SERVICE_ID, Value, env, orchestrator,
};

#[test]
fn agent_evidence_harness_outcome_records_validation_review_and_repair_signals() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.record_run_attempt("run-harness", "issue-harness", 2, "failed")
		.expect("run should persist");

	record_harness_signal_fixture_events(&state_store);

	let recorded = orchestrator::record_harness_outcome_for_issue_run(
		&state_store,
		HarnessOutcomeRecordInput {
			project_id: TEST_SERVICE_ID,
			issue_id: "issue-harness",
			issue_identifier: "PUB-110",
			run_id: "run-harness",
			attempt_number: 2,
			outcome: HarnessOutcomeKind::RetryableFailure,
			error_class: Some("repo_gate_verify_failed"),
			validation_result: Some("failed"),
			pr_url: None,
		},
	)
	.expect("harness outcome should record");
	let payload = recorded.payload();

	assert_eq!(recorded.event_type(), "harness_outcome");

	assert_harness_outcome_payload(payload);

	let request = EvidenceRequest {
		config_path: None,
		project_id: None,
		issue: "issue-harness",
		run_id: Some("run-harness"),
		attempt_number: Some(2),
		json: true,
		include_payload: false,
	};
	let readback = orchestrator::build_private_evidence_readback(&state_store, &config, &request)
		.expect("private evidence should read");

	assert_harness_private_readback(&readback);
}

#[test]
fn harness_outcome_does_not_report_validation_failed_for_review_no_effective_diff() {
	let (_temp_dir, _config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.record_run_attempt("run-review-stalled", "issue-review-stalled", 1, "failed")
		.expect("run should persist");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-review-stalled",
			"run-review-stalled",
			1,
			"review_checkpoint",
			serde_json::json!({
				"phase": "handoff",
				"status": "findings",
				"head_sha": "abc123",
				"nonclean_rounds": 2,
				"review": {
					"accepted_findings": [
						{"summary": "remove stale storage docs"},
						{"summary": "update runbook handoff evidence"}
					],
					"rejected_findings": []
				}
			}),
		)
		.expect("review checkpoint should append");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-review-stalled",
			"run-review-stalled",
			1,
			"loop_guardrail_checkpoint",
			serde_json::json!({
				"schema": "decodex.loop_guardrail_checkpoint/1",
				"reason": "no_effective_diff",
				"consecutive_count": 1,
				"threshold": 3,
				"source_error_class": null,
				"details": {
					"effective_delta_present": false
				}
			}),
		)
		.expect("guardrail checkpoint should append");

	let recorded = orchestrator::record_harness_outcome_for_issue_run(
		&state_store,
		HarnessOutcomeRecordInput {
			project_id: TEST_SERVICE_ID,
			issue_id: "issue-review-stalled",
			issue_identifier: "PUB-1579",
			run_id: "run-review-stalled",
			attempt_number: 1,
			outcome: HarnessOutcomeKind::RetryableFailure,
			error_class: Some("retryable_execution_failure"),
			validation_result: None,
			pr_url: None,
		},
	)
	.expect("harness outcome should record");
	let payload = recorded.payload();

	assert_eq!(payload["validation"]["result"], "not_recorded");
	assert_eq!(payload["validation"]["failure_count"], 0);
	assert_eq!(payload["validation"]["failure_classes"], serde_json::json!([]));
	assert_eq!(payload["manual_attention"], Value::Null);

	assert_harness_payload_candidate(payload, "review_repair_no_effective_diff_after_findings");
}

fn assert_harness_outcome_payload(payload: &Value) {
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

	assert_harness_payload_candidate(payload, "accepted_review_findings");
	assert_harness_payload_candidate(payload, "authority_underspecified");
	assert_harness_payload_candidate(payload, "architecture_recovery_exhausted");
}

fn assert_harness_payload_candidate(payload: &Value, reason_code: &str) {
	assert!(
		payload["improvement_candidates"]
			.as_array()
			.expect("candidates should be an array")
			.iter()
			.any(|candidate| candidate["reason_code"] == reason_code)
	);
}

fn assert_harness_private_readback(readback: &PrivateEvidenceReadback) {
	assert!(
		readback
			.improvement_candidates
			.iter()
			.any(|candidate| candidate.reason_code == "accepted_review_findings")
	);
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
	assert!(readback.phase_acceptance_checks.iter().any(|check| {
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
	assert!(rendered.contains("Phase Acceptance Checks"));
	assert!(rendered.contains("Architecture Recoveries"));
	assert!(rendered.contains("Boundary Checks"));
	assert!(rendered.contains("status: findings"));
	assert!(rendered.contains("reason_code: no_effective_delta"));
	assert!(rendered.contains("disposition: requires_human"));
	assert!(readback.events.iter().all(|event| event.payload.is_none()));
}

fn record_harness_signal_fixture_events(state_store: &StateStore) {
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
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-harness",
			"run-harness",
			2,
			"progress_checkpoint",
			serde_json::json!({
				"phase": "review_repair",
				"focus": "repair accepted finding"
			}),
		)
		.expect("progress evidence should append");

	record_harness_phase_acceptance_event(state_store);

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

fn record_harness_phase_acceptance_event(state_store: &StateStore) {
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-harness",
			"run-harness",
			2,
			PHASE_ACCEPTANCE_CHECK_EVENT_TYPE,
			serde_json::json!({
				"schema": "decodex.phase_acceptance_check/1",
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
		.expect("phase acceptance evidence should append");
}

#[test]
fn harness_eval_fixture_recommends_contract_improvement_without_payload_leakage() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let fixture: Value = serde_json::from_str(include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/fixtures/harness_improvement/incomplete_contract_eval.json"
	)))
	.expect("harness eval fixture should parse");
	let issue_id = fixture["issue_id"].as_str().expect("fixture issue id");
	let issue_identifier = fixture["issue_identifier"].as_str().expect("fixture issue identifier");
	let run_id = fixture["run_id"].as_str().expect("fixture run id");
	let attempt_number = fixture["attempt_number"].as_i64().expect("fixture attempt");
	let contract: DecisionContract = serde_json::from_value(fixture["decision_contract"].clone())
		.expect("fixture contract should deserialize");

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			issue_id,
			"y/decodex-xy-857-eval",
			".worktrees/XY-857-EVAL",
		)
		.expect("worktree should persist");
	state_store
		.record_run_attempt(run_id, issue_id, attempt_number, "terminal_guarded")
		.expect("run should persist");
	state_store
		.upsert_decision_contract(TEST_SERVICE_ID, Some(issue_id), contract)
		.expect("contract should persist");

	for event in fixture["private_events"].as_array().expect("fixture events") {
		state_store
			.append_private_execution_event(
				TEST_SERVICE_ID,
				issue_id,
				run_id,
				attempt_number,
				event["event_type"].as_str().expect("fixture event type"),
				event["payload"].clone(),
			)
			.expect("fixture event should append");
	}

	orchestrator::record_harness_outcome_for_issue_run(
		&state_store,
		HarnessOutcomeRecordInput {
			project_id: TEST_SERVICE_ID,
			issue_id,
			issue_identifier,
			run_id,
			attempt_number,
			outcome: HarnessOutcomeKind::ManualAttention,
			error_class: Some("uncovered_direction"),
			validation_result: None,
			pr_url: None,
		},
	)
	.expect("harness outcome should record");

	let request = EvidenceRequest {
		config_path: None,
		project_id: None,
		issue: issue_identifier,
		run_id: Some(run_id),
		attempt_number: Some(attempt_number),
		json: false,
		include_payload: false,
	};
	let readback = orchestrator::build_private_evidence_readback(&state_store, &config, &request)
		.expect("fixture readback should summarize private evidence");
	let expected = &fixture["expected_candidate"];

	assert!(readback.improvement_candidates.iter().any(|candidate| {
		candidate.kind == expected["kind"].as_str().expect("expected kind")
			&& candidate.reason_code == expected["reason_code"].as_str().expect("expected reason")
			&& candidate.target == expected["target"].as_str().expect("expected target")
	}));

	let rendered = orchestrator::render_private_evidence_readback(&readback);

	assert!(rendered.contains("Improvement Candidates"));
	assert!(rendered.contains("underspecified_decision_contract"));
	assert!(!rendered.contains("Decide how generated issues must cite"));
	assert!(readback.events.iter().all(|event| event.payload.is_none()));
}
