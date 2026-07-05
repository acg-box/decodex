use crate::agent::tracker_tool_bridge::{
	DynamicToolContentItem, DynamicToolHandler, ISSUE_COMMENT_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME,
	TrackerToolBridge,
	tests::{self, FakeLocalRepoInspector, FakePullRequestInspector},
};

#[test]
fn rejects_manual_attention_comment_with_non_public_error_class_shape() {
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
	let mut args = tests::manual_attention_comment_args();

	args["error_class"] = serde_json::json!("Missing-GITHUB_PAT_Y");

	let response = DynamicToolHandler::handle_call(&bridge, ISSUE_COMMENT_TOOL_NAME, args);

	assert!(label_response.success);
	assert!(!response.success);
	assert!(tracker.comments.borrow().is_empty());
	assert!(
		tracker.label_additions.borrow().is_empty(),
		"invalid error_class shape must not leave a needs-attention label"
	);
	assert_eq!(
		response.content_items,
		vec![DynamicToolContentItem::InputText {
			text: String::from("`error_class` must be a public snake_case identifier.")
		}]
	);
}
