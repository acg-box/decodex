use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolHandler, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker,
	ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME, TempDir, TrackerToolBridge,
	review_policy,
};

#[test]
fn review_checkpoint_rejected_finding_is_non_actionable_and_can_handoff_cleanly() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(vec![Ok(tests::sample_pull_request())]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
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
			"evidence": ["only rejected non-actionable feedback remained"],
			"rejected_findings": [{
				"severity": "low",
				"summary": "The reviewer requested a migration test.",
				"rejection_reason": "No migration path changed in the current diff.",
				"evidence": ["The runtime store column is additive and defaults existing rows."],
				"file": "apps/decodex/src/state/internal.rs",
				"line": 1
			}]
		}),
	);

	assert!(response.success);

	let handoff_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": "https://github.com/hack-ink/decodex/pull/48",
			"summary": "Rejected non-actionable review feedback and prepared handoff."
		}),
	);

	assert!(handoff_response.success);

	tests::assert_review_policy_checkpoint_cleared(&bridge, &issue, &review_context);
}
