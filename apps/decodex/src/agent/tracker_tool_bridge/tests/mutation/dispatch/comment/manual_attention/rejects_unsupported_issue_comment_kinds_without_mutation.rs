use crate::agent::tracker_tool_bridge::{
	DynamicToolContentItem, DynamicToolHandler, ISSUE_COMMENT_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME,
	TrackerToolBridge,
	tests::{self, FakeLocalRepoInspector, FakePullRequestInspector},
};

#[test]
fn rejects_unsupported_issue_comment_kinds_without_mutation() {
	let issue = tests::sample_issue();
	let tracker = tests::tracker_with_current_issue_snapshot(&issue);
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		tests::sample_review_context(),
		&inspector,
		&local_repo_inspector,
	);
	let label_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_LABEL_ADD_TOOL_NAME,
		serde_json::json!({ "label": "decodex:needs-attention" }),
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_COMMENT_TOOL_NAME,
		serde_json::json!({
			"kind": "status_note",
			"error_class": "operator_decision_required",
			"next_action": "resolve manually",
			"blockers": ["operator decision is required"],
			"evidence": ["agent attempted unsupported comment kind"]
		}),
	);

	assert!(label_response.success);
	assert!(!response.success);
	assert!(tracker.comments.borrow().is_empty());
	assert!(
		tracker.label_additions.borrow().is_empty(),
		"unsupported comment kind must not leave a needs-attention label"
	);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("Unsupported `issue_comment` kind `status_note`")
	));
}
