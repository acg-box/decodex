use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolContentItem, DynamicToolHandler, FakeLocalRepoInspector,
	FakePullRequestInspector, FakeTracker, ISSUE_REVIEW_HANDOFF_TOOL_NAME, TempDir,
	TrackerToolBridge,
};

#[test]
fn review_handoff_rejects_stale_clean_checkpoint_for_previous_head() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let mut updated_local_repo = tests::sample_local_repo();
	let mut updated_pull_request = tests::sample_pull_request();

	updated_local_repo.head_oid = String::from("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
	updated_pull_request.head_ref_oid = updated_local_repo.head_oid.clone();
	updated_pull_request.url = String::from("https://github.com/hack-ink/decodex/pull/149");

	let review_context = tests::sample_review_context_in(temp_dir.path());
	let inspector = FakePullRequestInspector::new(vec![Ok(updated_pull_request)]);
	let local_repo_inspector =
		FakeLocalRepoInspector::new(vec![Ok(updated_local_repo.clone()), Ok(updated_local_repo)]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);

	tests::write_review_policy_checkpoint(
		&bridge,
		&issue,
		&review_context,
		"handoff",
		"clean",
		&tests::sample_local_repo().head_oid,
		0,
	);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": "https://github.com/hack-ink/decodex/pull/149",
			"summary": "Ready for review."
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText{ text }]
			if text.contains("requires a current `handoff` review checkpoint with status `clean` for the current lane HEAD")
	));
}
