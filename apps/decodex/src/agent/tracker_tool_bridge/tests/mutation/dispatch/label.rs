use crate::agent::tracker_tool_bridge::{
	DynamicToolContentItem, DynamicToolHandler, ISSUE_LABEL_ADD_TOOL_NAME, TrackerToolBridge,
	tests::{self, FakeTracker},
};

#[test]
fn records_manual_attention_label_intent_without_linear_mutation() {
	let issue = tests::sample_issue();
	let tracker = FakeTracker::new();
	let workflow = tests::sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_LABEL_ADD_TOOL_NAME,
		serde_json::json!({ "label": "decodex:needs-attention" }),
	);

	assert!(response.success);
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("Manual-attention label intent recorded")
	));
}

#[test]
fn adds_allowed_workflow_opt_out_label() {
	let issue = tests::sample_issue();
	let tracker = tests::tracker_with_current_issue_snapshot(&issue);
	let workflow = tests::sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_LABEL_ADD_TOOL_NAME,
		serde_json::json!({ "label": "decodex:manual-only" }),
	);

	assert!(response.success);
	assert_eq!(tracker.label_additions.borrow().as_slice(), [vec![String::from("label-manual")]]);
}

#[test]
fn rejects_invalid_label_add_argument_shapes() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let cases = [
		(
			serde_json::json!({ "unexpected_label_field": "decodex:needs-attention" }),
			"Invalid `issue.label.add` arguments: missing field `label`",
		),
		(
			serde_json::json!({
				"label": "decodex:needs-attention",
				"unexpected_label_field": "decodex:needs-attention",
			}),
			"Invalid `issue.label.add` arguments: unknown field `unexpected_label_field`",
		),
	];

	for (arguments, message) in cases {
		let response =
			DynamicToolHandler::handle_call(&bridge, ISSUE_LABEL_ADD_TOOL_NAME, arguments);

		assert!(!response.success);
		assert_eq!(
			response.content_items,
			vec![DynamicToolContentItem::InputText { text: String::from(message) }]
		);
	}

	assert!(tracker.label_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
}
