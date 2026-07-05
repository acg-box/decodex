use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolContentItem, DynamicToolHandler, FakeLocalRepoInspector,
	FakePullRequestInspector, FakeTracker, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, TempDir,
	TrackerToolBridge, review_policy,
};

#[test]
fn compact_review_checkpoint_fails_closed_for_non_low_risk_classification() {
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
	let mut cost_control = review_policy::compact_review_cost_control_json();

	cost_control["risk_class"] = serde_json::json!("localized");

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "clean",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::handoff_review_contract_json(),
			"review_cost_control": cost_control,
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer tried to claim compact review for a localized-risk lane"]
		}),
	);

	assert!(!response.success);

	match &response.content_items[..] {
		[DynamicToolContentItem::InputText { text }] => {
			assert!(text.contains("full review is required"));
			assert!(text.contains("review_contract_risk_tier_not_low"));
			assert!(text.contains("review_cost_risk_class_not_low"));
		},
		other => panic!("unexpected response content: {other:?}"),
	}
}

#[test]
fn compact_review_checkpoint_fails_closed_for_accepted_findings_and_blocking_routes() {
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
			"review_contract": review_policy::low_risk_handoff_review_contract_json(),
			"review_cost_control": review_policy::compact_review_cost_control_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer tried to keep compact review after accepting a current blocker"],
			"accepted_findings": review_policy::accepted_review_findings_json(),
			"finding_routes": [{
				"route": "current_blocker",
				"severity": "medium",
				"risk_tier": "medium",
				"summary": "Accepted current repair blocker.",
				"evidence": ["The accepted finding applies to the current lane head."],
				"resolver": "agent",
				"next_action": "Repair the accepted finding before handoff.",
				"finding_source": "accepted_findings",
				"finding_index": 0
			}]
		}),
	);

	assert!(!response.success);

	match &response.content_items[..] {
		[DynamicToolContentItem::InputText { text }] => {
			assert!(text.contains("full review is required"));
			assert!(text.contains("accepted_findings_present"));
			assert!(text.contains("blocking_finding_routes_present"));
			assert!(text.contains("nonclean_review_status"));
		},
		other => panic!("unexpected response content: {other:?}"),
	}
}
