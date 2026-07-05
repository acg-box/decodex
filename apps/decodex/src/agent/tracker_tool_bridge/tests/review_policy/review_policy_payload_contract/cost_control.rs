use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolHandler, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker,
	ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, TempDir, TrackerToolBridge, Value, review_policy,
};

#[test]
fn compact_review_checkpoint_persists_low_risk_cost_control() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "clean",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::low_risk_handoff_review_contract_json(),
			"review_cost_control": review_policy::compact_review_cost_control_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer checked compact eligibility against current HEAD and validation evidence"]
		}),
	);

	assert!(response.success);

	let checkpoint = tests::persisted_review_policy_checkpoint(&bridge, &issue, &review_context);
	let details =
		serde_json::from_str::<Value>(checkpoint.details_json()).expect("details should be json");

	assert_eq!(details["review_cost_control"]["review_class"], "compact_current_head_review");
	assert_eq!(details["review_cost_control"]["risk_class"], "low");
	assert_eq!(details["review_cost_control"]["compact_eligible"], true);
	assert_eq!(details["review_cost_control"]["validation_current"], true);
	assert_eq!(details["review_cost_control"]["evidence_sufficient"], true);
	assert!(details["review_cost_control"]["fallback_reason"].is_null());

	let events = tests::bridge_state_store(&bridge)
		.list_private_execution_events_for_run_attempt(
			&review_context.service_id,
			&review_context.run_id,
			review_context.attempt_number,
		)
		.expect("private review evidence should read");

	assert_eq!(events[0].payload()["review_class"], "compact_current_head_review");
	assert_eq!(events[0].payload()["risk_class"], "low");
	assert_eq!(events[0].payload()["compact_eligible"], true);
}

#[test]
fn full_review_checkpoint_persists_cost_control_fallback_reason() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "clean",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::handoff_review_contract_json(),
			"review_cost_control": review_policy::full_review_cost_control_json("operator_facing_runtime_review_behavior_changed"),
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer kept standard full review because runtime review behavior changed"]
		}),
	);

	assert!(response.success);

	let checkpoint = tests::persisted_review_policy_checkpoint(&bridge, &issue, &review_context);
	let details =
		serde_json::from_str::<Value>(checkpoint.details_json()).expect("details should be json");

	assert_eq!(details["review_cost_control"]["review_class"], "full_current_head_review");
	assert_eq!(
		details["review_cost_control"]["fallback_reason"],
		"operator_facing_runtime_review_behavior_changed"
	);
	assert_eq!(details["review_cost_control"]["compact_eligible"], false);
}
