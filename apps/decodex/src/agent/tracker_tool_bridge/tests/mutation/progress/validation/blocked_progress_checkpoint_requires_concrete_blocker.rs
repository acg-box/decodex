use crate::agent::tracker_tool_bridge::{
	DynamicToolContentItem, DynamicToolHandler, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
	TrackerToolBridge,
	tests::{self, FakeTracker},
};

#[test]
fn blocked_progress_checkpoint_requires_concrete_blocker() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"phase": "blocked",
				"docs_impact": "none",
			"focus": "Unblock closeout.",
			"next_action": "Wait for a blocker to be clarified.",
			"blockers": [],
			"evidence": []
		}),
	);

	assert!(!response.success);
	assert!(
		response
			.content_items
			.iter()
			.any(|item| matches!(item, DynamicToolContentItem::InputText { text } if text.contains("requires at least one blocker")))
	);
	assert!(tracker.comments.borrow().is_empty());
}
