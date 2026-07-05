use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolContentItem, DynamicToolHandler, FakeLocalRepoInspector,
	FakePullRequestInspector, FakeTracker, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
	LocalRepoDetails, PullRequestDetails, ReviewHandoffMarker, TEST_SERVICE_ID, TempDir,
	TrackerToolBridge,
};

#[test]
fn review_repair_complete_requires_a_clean_checkpoint() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/242";
	let inspector = FakePullRequestInspector::new(vec![Ok(PullRequestDetails {
		head_ref_name: String::from("x/decodex-pub-618"),
		head_ref_oid: String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		head_repository_name: String::from("decodex"),
		head_repository_owner: String::from("hack-ink"),
		is_draft: false,
		state: String::from("OPEN"),
		base_ref_name: String::from("main"),
		url: String::from(pr_url),
	})]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(LocalRepoDetails {
		default_branch: String::from("main"),
		head_oid: String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		head_tree_oid: String::from("f8a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		repository_name: String::from("decodex"),
		repository_owner: String::from("hack-ink"),
		review_blocking_changes: Vec::new(),
	})]);
	let review_context = tests::sample_review_repair_context_in(temp_dir.path(), pr_url);
	let bridge = TrackerToolBridge::with_review_repair_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);

	tests::bridge_state_store(&bridge)
		.upsert_review_handoff_marker(
			TEST_SERVICE_ID,
			&issue.id,
			&ReviewHandoffMarker::new(
				String::from("pub-618-attempt-2-100"),
				2,
				review_context.branch_name.clone(),
				String::from(pr_url),
				String::from("main"),
				review_context.branch_name.clone(),
				String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			),
		)
		.expect("original review handoff marker should persist");

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
		serde_json::json!({
			"pr_url": pr_url,
			"summary": "Ready for fresh review."
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText{ text }]
			if text.contains("requires a current `repair` review checkpoint with status `clean`")
	));
}
