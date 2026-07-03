use crate::agent::tracker_tool_bridge::{
	DynamicToolContentItem, DynamicToolHandler, ISSUE_COMMENT_TOOL_NAME, TrackerToolBridge,
	tests::{self, FakeTracker},
};

#[test]
fn rejects_tool_calls_for_another_issue() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let mut args = tests::manual_attention_comment_args();

	args["issue_identifier"] = serde_json::json!("DEC-999");

	let response = DynamicToolHandler::handle_call(&bridge, ISSUE_COMMENT_TOOL_NAME, args);

	assert!(!response.success);
	assert!(tracker.comments.borrow().is_empty());
}

#[test]
fn rejects_arbitrary_issue_comment_bodies() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_COMMENT_TOOL_NAME,
		serde_json::json!({ "body": "Started work and running validation now." }),
	);

	assert!(!response.success);
	assert!(tracker.comments.borrow().is_empty());
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("Invalid `issue.comment` arguments")
	));
}
