use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolContentItem, DynamicToolHandler, ISSUE_LABEL_ADD_TOOL_NAME, TrackerToolBridge,
		tests::{self, FakeTracker},
	},
	tracker::TrackerLabel,
};

#[test]
fn opt_out_label_add_uses_refreshed_issue_snapshot_for_label_ids() {
	let initial_issue = tests::sample_issue();
	let mut refreshed_issue = initial_issue.clone();

	refreshed_issue.labels.push(TrackerLabel {
		id: String::from("label-needs"),
		name: String::from("decodex:needs-attention"),
	});

	let tracker = FakeTracker::with_refresh_snapshots(vec![vec![refreshed_issue]]);
	let workflow = tests::sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &initial_issue, &workflow);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_LABEL_ADD_TOOL_NAME,
		serde_json::json!({ "label": "decodex:manual-only" }),
	);

	assert!(response.success);
	assert_eq!(tracker.label_additions.borrow().as_slice(), [vec![String::from("label-manual")]]);
}

#[test]
fn label_add_fails_when_refresh_returns_no_snapshot() {
	let tracker = FakeTracker::with_refresh_snapshots(vec![Vec::new()]);
	let workflow = tests::sample_workflow();
	let issue = tests::sample_issue();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_LABEL_ADD_TOOL_NAME,
		serde_json::json!({ "label": "decodex:manual-only" }),
	);

	assert!(!response.success);
	assert_eq!(
		response.content_items,
		vec![DynamicToolContentItem::InputText {
			text: format!(
				"Failed to refresh issue `{}` before updating labels: tracker returned no current snapshot.",
				issue.identifier
			),
		}]
	);
	assert!(tracker.label_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
}
