use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolContentItem, DynamicToolHandler, FakeLocalRepoInspector,
	FakePullRequestInspector, FakeTracker, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, TempDir,
	TrackerToolBridge, review_policy,
};

#[test]
fn compact_review_checkpoint_fails_closed_with_stale_validation_evidence() {
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

	cost_control["validation_current"] = serde_json::json!(false);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "clean",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::low_risk_handoff_review_contract_json(),
			"review_cost_control": cost_control,
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer tried to claim compact review with validation from an older head"]
		}),
	);

	assert!(!response.success);

	match &response.content_items[..] {
		[DynamicToolContentItem::InputText { text }] => {
			assert!(text.contains("full review is required"));
			assert!(text.contains("stale_validation_evidence"));
		},
		other => panic!("unexpected response content: {other:?}"),
	}
}

#[test]
fn compact_review_checkpoint_fails_closed_with_weak_evidence() {
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

	cost_control["evidence_sufficient"] = serde_json::json!(false);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "clean",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::low_risk_handoff_review_contract_json(),
			"review_cost_control": cost_control,
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer tried to claim compact review with weak current-head evidence"]
		}),
	);

	assert!(!response.success);

	match &response.content_items[..] {
		[DynamicToolContentItem::InputText { text }] => {
			assert!(text.contains("full review is required"));
			assert!(text.contains("weak_evidence"));
		},
		other => panic!("unexpected response content: {other:?}"),
	}
}
