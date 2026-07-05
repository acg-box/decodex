use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolHandler, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker,
	ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, TempDir, TrackerToolBridge, TurnCompletionStatus, Value,
	review_policy,
};

#[test]
fn independent_review_checkpoint_findings_store_accepted_repair_guidance() {
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
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "findings",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::handoff_review_contract_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer found one accepted repair item"],
			"accepted_findings": review_policy::accepted_review_findings_json()
		}),
	);

	assert!(response.success);
	assert_eq!(
		DynamicToolHandler::classify_turn_completion(&bridge, "continue")
			.expect("first accepted findings round should continue"),
		TurnCompletionStatus::Continue
	);

	let checkpoint = tests::persisted_review_policy_checkpoint(&bridge, &issue, &review_context);
	let details =
		serde_json::from_str::<Value>(checkpoint.details_json()).expect("details should be json");

	assert_eq!(checkpoint.status(), "findings");
	assert_eq!(checkpoint.nonclean_rounds(), 1);
	assert_eq!(details["accepted_findings"][0]["severity"], "medium");
	assert_eq!(details["accepted_findings"][0]["kind"], "accepted_finding");
	assert_eq!(details["accepted_findings"][0]["line_range"]["start"], 1);
	assert!(
		details["accepted_findings"][0]["fingerprint"]
			.as_str()
			.is_some_and(|fingerprint| fingerprint.starts_with("review_finding:"))
	);
	assert_eq!(
		details["accepted_findings"][0]["guidance"],
		"Repair the accepted issue before requesting another review checkpoint."
	);
	assert_eq!(details["finding_routes"][0]["route"], "current_blocker");
	assert_eq!(
		details["finding_routes"][0]["finding_fingerprint"],
		details["accepted_findings"][0]["fingerprint"]
	);
	assert_eq!(details["finding_route_summary"]["route_counts"][0]["route"], "current_blocker");
}
