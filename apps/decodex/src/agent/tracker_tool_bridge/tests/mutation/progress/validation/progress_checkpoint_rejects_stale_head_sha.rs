use tempfile::TempDir;

use crate::agent::tracker_tool_bridge::{
	DynamicToolContentItem, DynamicToolHandler, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
	TrackerToolBridge,
	tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker},
};

#[test]
fn progress_checkpoint_rejects_stale_head_sha() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		tests::sample_review_context_in(temp_dir.path()),
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"phase": "implementing",
				"openwiki_impact": "none",
			"focus": "Keep execution state tied to the current lane head.",
			"next_action": "Reject stale checkpoint writes.",
			"blockers": [],
			"evidence": [],
			"head_sha": "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
		}),
	);

	assert!(!response.success);
	assert!(
		response
			.content_items
			.iter()
			.any(|item| matches!(item, DynamicToolContentItem::InputText { text } if text.contains("does not match the current lane HEAD")))
	);
	assert!(tracker.comments.borrow().is_empty());
}
