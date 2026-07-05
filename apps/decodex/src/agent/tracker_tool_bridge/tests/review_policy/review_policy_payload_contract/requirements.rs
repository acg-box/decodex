use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolContentItem, DynamicToolHandler, FakeLocalRepoInspector,
	FakePullRequestInspector, FakeTracker, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, TempDir,
	TrackerToolBridge, review_policy,
};

#[test]
fn independent_review_checkpoint_requires_structured_fresh_context_payload() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
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

	for (payload, expected_error) in [
		(
			serde_json::json!({
				"status": "clean",
				"head_sha": tests::sample_local_repo().head_oid,
				"checks": review_policy::review_checks_json(),
				"evidence": ["review evidence"]
			}),
			"requires `reviewer`",
		),
		(
			serde_json::json!({
				"reviewer": "self_review",
				"status": "clean",
				"head_sha": tests::sample_local_repo().head_oid,
				"checks": review_policy::review_checks_json(),
				"evidence": ["review evidence"]
			}),
			"reviewer must be `independent_fresh_context`",
		),
		(
			serde_json::json!({
				"reviewer": "independent_fresh_context",
				"status": "clean",
				"head_sha": tests::sample_local_repo().head_oid,
				"review_contract": review_policy::handoff_review_contract_json(),
				"evidence": ["review evidence"]
			}),
			"requires `checks`",
		),
		(
			serde_json::json!({
				"reviewer": "independent_fresh_context",
				"status": "clean",
				"head_sha": tests::sample_local_repo().head_oid,
				"review_contract": review_policy::handoff_review_contract_json(),
				"checks": review_policy::review_checks_json(),
				"evidence": []
			}),
			"requires `evidence`",
		),
		(
			serde_json::json!({
				"reviewer": "independent_fresh_context",
				"status": "findings",
				"head_sha": tests::sample_local_repo().head_oid,
				"review_contract": review_policy::handoff_review_contract_json(),
				"checks": review_policy::review_checks_json(),
				"evidence": ["review evidence"],
				"accepted_findings": [{
					"severity": "medium",
					"summary": "Accepted reviewer finding",
					"evidence": [],
					"guidance": "Repair the accepted issue before requesting another checkpoint."
				}]
			}),
			"requires `accepted_findings.evidence`",
		),
		(
			serde_json::json!({
				"reviewer": "independent_fresh_context",
				"status": "clean",
				"head_sha": tests::sample_local_repo().head_oid,
				"review_contract": review_policy::handoff_review_contract_json(),
				"checks": review_policy::review_checks_json(),
				"evidence": ["review evidence"],
				"rejected_findings": [{
					"severity": "unknown",
					"summary": "Rejected reviewer finding",
					"rejection_reason": "Not actionable after validation.",
					"evidence": ["Reviewer evidence was stale."]
				}]
			}),
			"`rejected_findings.severity` must be",
		),
	] {
		let response =
			DynamicToolHandler::handle_call(&bridge, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, payload);

		assert!(!response.success);
		assert!(matches!(
			response.content_items.as_slice(),
			[DynamicToolContentItem::InputText { text }] if text.contains(expected_error)
		));
	}
}

#[test]
fn independent_review_checkpoint_requires_review_contract() {
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
			"checks": review_policy::review_checks_json(),
			"evidence": ["review evidence"]
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }] if text.contains("requires `review_contract`")
	));
}
