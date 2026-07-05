use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolContentItem, DynamicToolHandler, FakeLocalRepoInspector,
	FakePullRequestInspector, FakeTracker, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, TempDir,
	TrackerToolBridge, review_policy,
};

#[test]
fn compact_review_checkpoint_fails_closed_after_prior_nonclean_round() {
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
		review_context,
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let findings_response = review_policy::submit_findings_review_checkpoint(
		&bridge,
		"first full review found a current blocker",
	);

	assert!(findings_response.success);

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
			"evidence": ["fresh reviewer confirmed the accepted finding was repaired"]
		}),
	);

	assert!(!response.success);

	match &response.content_items[..] {
		[DynamicToolContentItem::InputText { text }] => {
			assert!(text.contains("prior_nonclean_review_rounds_present"));
		},
		other => panic!("unexpected response content: {other:?}"),
	}
}

#[test]
fn compact_review_checkpoint_fails_closed_after_prior_nonclean_round_on_repaired_head() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let mut repaired_local_repo = tests::sample_local_repo();

	repaired_local_repo.head_oid = String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	repaired_local_repo.head_tree_oid = String::from("28a20f7dfb9526e7421a5f095b1c6adec84e52d6");

	let repaired_head = repaired_local_repo.head_oid.clone();
	let local_repo_inspector =
		FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo()), Ok(repaired_local_repo)]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context,
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let findings_response = review_policy::submit_findings_review_checkpoint(
		&bridge,
		"first full review found a current blocker",
	);

	assert!(findings_response.success);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "clean",
			"head_sha": repaired_head,
			"review_contract": review_policy::low_risk_handoff_review_contract_json(),
			"review_cost_control": review_policy::compact_review_cost_control_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer confirmed the accepted finding was repaired on a new head"]
		}),
	);

	assert!(!response.success);

	match &response.content_items[..] {
		[DynamicToolContentItem::InputText { text }] => {
			assert!(text.contains("prior_nonclean_review_rounds_present"));
		},
		other => panic!("unexpected response content: {other:?}"),
	}
}
