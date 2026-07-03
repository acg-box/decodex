use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolHandler, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker,
	ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, TempDir, TrackerToolBridge, TurnCompletionStatus, Value,
	review_policy,
};

#[test]
fn repair_review_checkpoint_stores_accepted_findings_for_repair_loop() {
	let tracker = FakeTracker::new();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let repair_context = tests::sample_review_repair_context_in(
		temp_dir.path(),
		"https://github.com/hack-ink/decodex/pull/242",
	);
	let issue = tests::sample_review_issue();
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_repair_for_test(
		&tracker,
		&issue,
		&workflow,
		repair_context.clone(),
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
			"review_contract": review_policy::repair_review_contract_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh-context retained repair review accepted one finding"],
			"accepted_findings": review_policy::accepted_review_findings_json(),
			"rejected_findings": [{
				"severity": "info",
				"summary": "Reviewer suggested changing unrelated landing code.",
				"rejection_reason": "Outside this retained repair batch.",
				"evidence": ["The current PR feedback only concerns the tracker-tool bridge."]
			}]
		}),
	);

	assert!(response.success);

	let checkpoint = tests::persisted_review_policy_checkpoint(&bridge, &issue, &repair_context);
	let details =
		serde_json::from_str::<Value>(checkpoint.details_json()).expect("details should be json");

	assert_eq!(checkpoint.phase(), "repair");
	assert_eq!(details["review_contract"]["review_type"], "repair_verification");
	assert_eq!(details["accepted_findings"][0]["summary"], "Accepted reviewer finding");
	assert_eq!(
		details["rejected_findings"][0]["rejection_reason"],
		"Outside this retained repair batch."
	);
}

#[test]
fn stale_review_checkpoint_for_old_head_does_not_stop_new_head() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let mut updated_local_repo = tests::sample_local_repo();

	updated_local_repo.head_oid = String::from("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");

	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(updated_local_repo)]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&pull_request_inspector,
		&local_repo_inspector,
	);

	tests::write_review_policy_checkpoint(
		&bridge,
		&issue,
		&review_context,
		"handoff",
		"blocked",
		&tests::sample_local_repo().head_oid,
		0,
	);

	assert_eq!(
		DynamicToolHandler::classify_turn_completion(&bridge, "continue")
			.expect("a stale checkpoint from an older head should be ignored"),
		TurnCompletionStatus::Continue
	);
}
