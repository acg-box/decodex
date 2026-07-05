use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolContentItem, DynamicToolHandler, FakeLocalRepoInspector,
	FakePullRequestInspector, FakeTracker, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, TempDir,
	TrackerToolBridge, review_policy,
};

#[test]
fn review_checkpoint_rejects_out_of_range_rejected_route_binding() {
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
			"status": "clean",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::handoff_review_contract_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer tried to bind a route to a missing rejected finding"],
			"rejected_findings": [{
				"severity": "low",
				"summary": "Reviewer requested unrelated follow-up work.",
				"rejection_reason": "The request is outside the current issue contract.",
				"evidence": ["The current diff does not touch that surface."]
			}],
			"finding_routes": [{
				"route": "reviewer_rubric_gap",
				"severity": "low",
				"risk_tier": "low",
				"summary": "Out-of-range rejected finding binding.",
				"evidence": ["Only one rejected finding exists."],
				"resolver": "reviewer",
				"next_action": "Bind to an existing rejected finding index.",
				"finding_source": "rejected_findings",
				"finding_index": 99
			}]
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("`finding_routes.finding_index` `99` does not match any rejected finding")
	));
}
