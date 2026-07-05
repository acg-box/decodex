use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolHandler, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker,
	ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, TempDir, TrackerToolBridge, Value, review_policy,
};

#[test]
fn independent_review_checkpoint_clean_persists_structured_payload() {
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
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer read the issue contract, current diff, and HEAD"],
			"rejected_findings": [{
				"severity": "low",
				"summary": "The reviewer asked for a migration note, but no schema or data migration changed.",
				"rejection_reason": "Not actionable after checking the current diff and docs.",
				"evidence": ["Only runtime review checkpoint metadata changed."],
				"file": "apps/decodex/src/agent/tracker_tool_bridge/tools.rs",
				"line": 1
			}]
		}),
	);

	assert!(response.success);

	let checkpoint = tests::persisted_review_policy_checkpoint(&bridge, &issue, &review_context);
	let details =
		serde_json::from_str::<Value>(checkpoint.details_json()).expect("details should be json");

	assert_eq!(checkpoint.status(), "clean");
	assert_eq!(details["reviewer"], "independent_fresh_context");
	assert_eq!(details["review_contract"]["workflow_policy_source"], "registered_project_workflow");
	assert_eq!(details["review_contract"]["review_type"], "full_current_head_review");
	assert_eq!(details["review_cost_control"]["review_class"], "full_current_head_review");
	assert_eq!(
		details["review_cost_control"]["fallback_reason"],
		"review_cost_control_not_provided"
	);
	assert_eq!(details["reviewed_head"]["head_sha"], tests::sample_local_repo().head_oid);
	assert_eq!(details["reviewed_head"]["head_tree_oid"], tests::sample_local_repo().head_tree_oid);
	assert_eq!(details["reviewed_head"]["review_worktree_clean"], true);
	assert!(
		details["review_contract_hash"]
			.as_str()
			.is_some_and(|hash| hash.starts_with("review_contract:"))
	);
	assert_eq!(
		details["checks"]["loop_decision_contract"],
		"Compared the change against the accepted Loop/Decision Contract and found no mismatch."
	);
	assert_eq!(details["accepted_findings"].as_array().expect("accepted findings array").len(), 0);
	assert_eq!(
		details["rejected_findings"][0]["rejection_reason"],
		"Not actionable after checking the current diff and docs."
	);
	assert_eq!(details["finding_routes"][0]["route"], "reviewer_rubric_gap");
	assert_eq!(details["finding_route_summary"]["route_counts"][0]["route"], "reviewer_rubric_gap");
	assert_eq!(details["finding_route_summary"]["route_counts"][0]["count"], 1);

	let events = tests::bridge_state_store(&bridge)
		.list_private_execution_events_for_run_attempt(
			&review_context.service_id,
			&review_context.run_id,
			review_context.attempt_number,
		)
		.expect("private review evidence should read");

	assert_eq!(events.len(), 1);
	assert_eq!(events[0].event_type(), "review_checkpoint");
	assert_eq!(events[0].payload()["review"]["reviewer"], "independent_fresh_context");
	assert_eq!(events[0].payload()["route_counts"][0]["route"], "reviewer_rubric_gap");
}
