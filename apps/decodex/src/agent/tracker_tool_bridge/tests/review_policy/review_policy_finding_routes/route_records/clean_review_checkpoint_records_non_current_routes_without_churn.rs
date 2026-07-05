use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolHandler, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker,
	ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, TempDir, TrackerToolBridge, Value, review_policy,
};

#[test]
fn clean_review_checkpoint_records_non_current_routes_without_churn() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&pull_request_inspector,
		&local_repo_inspector,
	);

	for _round in 0..2 {
		let response = DynamicToolHandler::handle_call(
			&bridge,
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			serde_json::json!({
				"reviewer": "independent_fresh_context",
				"status": "clean",
				"head_sha": tests::sample_local_repo().head_oid,
				"review_contract": review_policy::handoff_review_contract_json(),
				"checks": review_policy::review_checks_json(),
				"evidence": ["fresh reviewer found only non-current follow-up work"],
				"finding_routes": review_policy::route_only_review_route_json("follow_up")
			}),
		);

		assert!(response.success);
	}

	let checkpoint = tests::persisted_review_policy_checkpoint(&bridge, &issue, &review_context);
	let details =
		serde_json::from_str::<Value>(checkpoint.details_json()).expect("details should be json");

	assert_eq!(checkpoint.status(), "clean");
	assert_eq!(checkpoint.nonclean_rounds(), 0);
	assert_eq!(
		details["finding_policy"]["active_fingerprints"]
			.as_array()
			.expect("active fingerprints should be an array")
			.len(),
		0
	);
	assert_eq!(details["finding_route_summary"]["route_counts"][0]["route"], "follow_up");
}
