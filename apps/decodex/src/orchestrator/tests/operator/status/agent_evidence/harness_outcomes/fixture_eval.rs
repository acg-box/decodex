use crate::orchestrator::{
	self, HarnessOutcomeKind, HarnessOutcomeRecordInput,
	tests::operator::status::{
		self, DecisionContract, EvidenceRequest, StateStore, TEST_SERVICE_ID, Value,
	},
};

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
