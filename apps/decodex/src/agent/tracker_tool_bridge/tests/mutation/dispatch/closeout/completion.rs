use tempfile::TempDir;

use crate::agent::tracker_tool_bridge::{
	DynamicToolContentItem, DynamicToolHandler, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
	TrackerToolBridge,
	tests::{self, FakeLocalRepoInspector, FakePullRequestInspector},
};

#[test]
fn closeout_complete_rejects_issue_that_is_not_yet_completed() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = tests::tracker_with_current_issue_snapshot(&tests::sample_review_issue());
	let issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/261";
	let mut merged_pull_request = tests::sample_pull_request();

	merged_pull_request.url = String::from(pr_url);
	merged_pull_request.state = String::from("MERGED");

	let inspector = FakePullRequestInspector::new(vec![
		Ok(merged_pull_request.clone()),
		Ok(merged_pull_request),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let bridge = TrackerToolBridge::with_review_handoff_inspectors(
		&tracker,
		&issue,
		&workflow,
		tests::sample_closeout_context_in(temp_dir.path(), pr_url),
		Some(TrackerToolBridge::leaked_test_state_store()),
		&inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
		serde_json::json!({
			"pr_url": pr_url,
			"summary": "Merged the approved lane and attempted closeout."
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("requires tracker state `Done`")
				&& text.contains("Move the issue to `Done` with `issue_transition` before calling `issue_closeout_complete`")
	));
}
