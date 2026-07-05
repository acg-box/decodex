use tempfile::TempDir;

use crate::agent::tracker_tool_bridge::{
	DynamicToolContentItem, DynamicToolHandler, ISSUE_REVIEW_HANDOFF_TOOL_NAME,
	ISSUE_TERMINAL_FINALIZE_TOOL_NAME, PullRequestDetails, TrackerToolBridge,
	tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker},
};

#[test]
fn terminal_finalize_rejects_mismatched_path() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(vec![Ok(PullRequestDetails {
		head_ref_name: String::from("x/decodex-pub-618"),
		head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		head_repository_name: String::from("decodex"),
		head_repository_owner: String::from("hack-ink"),
		is_draft: false,
		state: String::from("OPEN"),
		base_ref_name: String::from("main"),
		url: String::from("https://github.com/hack-ink/decodex/pull/54"),
	})]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);

	tests::write_clean_review_checkpoint(&bridge, &issue, &review_context);

	let review_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": "https://github.com/hack-ink/decodex/pull/54",
			"summary": "Ready for review."
		}),
	);
	let finalize_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		serde_json::json!({ "path": "manual_attention" }),
	);

	assert!(review_response.success);
	assert!(!finalize_response.success);
	assert!(matches!(
		finalize_response.content_items.as_slice(),
		[DynamicToolContentItem::InputText{ text }]
			if text.contains(
				"requested path `manual_attention`, but the recorded terminal path is `review_handoff`"
			)
	));
}
