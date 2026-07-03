use crate::orchestrator::{
	self, HarnessOutcomeKind, HarnessOutcomeRecordInput,
	tests::operator::status::{
		self, EvidenceRequest, StateStore, TEST_SERVICE_ID, Value,
		agent_evidence::harness_outcomes::support,
	},
};

#[test]
fn agent_evidence_harness_outcome_records_validation_review_and_repair_signals() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.record_run_attempt("run-harness", "issue-harness", 2, "failed")
		.expect("run should persist");

	support::record_harness_signal_fixture_events(&state_store);

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

	support::assert_harness_outcome_payload(payload);

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

	support::assert_harness_private_readback(&readback);
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

	support::assert_harness_payload_candidate(
		payload,
		"review_repair_no_effective_diff_after_findings",
	);
}
