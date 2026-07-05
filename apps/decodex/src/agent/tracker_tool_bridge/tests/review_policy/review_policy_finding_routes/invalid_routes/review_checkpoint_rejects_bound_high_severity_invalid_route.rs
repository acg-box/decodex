use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolContentItem, DynamicToolHandler, FakeLocalRepoInspector,
	FakePullRequestInspector, FakeTracker, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, TempDir,
	TrackerToolBridge, review_policy,
};

#[test]
fn review_checkpoint_rejects_bound_high_severity_invalid_route() {
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
		review_context,
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
			"evidence": ["fresh reviewer disputed a severe accepted finding"],
			"accepted_findings": [{
				"severity": "high",
				"summary": "Accepted reviewer finding reports a high severity regression.",
				"evidence": ["The reviewer evidence points at the current lane head."],
				"file": "apps/decodex/src/agent/tracker_tool_bridge/tools.rs",
				"line": 1,
				"guidance": "Repair the accepted high severity regression."
			}],
			"finding_routes": [{
				"route": "invalid_or_unsubstantiated",
				"severity": "low",
				"risk_tier": "low",
				"summary": "Route tries to downgrade the accepted finding.",
				"evidence": ["The bound accepted finding is high severity."],
				"resolver": "agent",
				"next_action": "Route to needs_evidence or a landing blocker instead.",
				"finding_source": "accepted_findings",
				"finding_index": 0
			}]
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("cannot route high-severity or high-risk")
	));
}
