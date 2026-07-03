use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolContentItem, DynamicToolHandler, ISSUE_TRANSITION_TOOL_NAME, TrackerToolBridge,
		tests::{self, FakeTracker},
	},
	tracker::TrackerIssue,
	workflow::WorkflowDocument,
};

#[test]
fn transitions_current_issue_with_allowed_state() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TRANSITION_TOOL_NAME,
		serde_json::json!({ "issue_identifier": "DEC-1", "state": "In Progress" }),
	);

	assert!(response.success);
	assert_eq!(tracker.state_updates.borrow().as_slice(), ["state-progress"]);
}

#[test]
fn rejects_success_transition_without_review_handoff() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TRANSITION_TOOL_NAME,
		serde_json::json!({ "state": "In Review" }),
	);

	assert!(!response.success);
	assert!(tracker.state_updates.borrow().is_empty());
	assert_eq!(
		response.content_items,
		vec![DynamicToolContentItem::InputText {
			text: String::from(
				"State `In Review` requires `issue_review_handoff` after the branch is pushed and a reviewable PR exists."
			),
		}]
	);
}

#[test]
fn rejects_invalid_transition_argument_shapes() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let bridge = TrackerToolBridge::new(&tracker, &issue, &workflow);
	let cases = [
		(
			serde_json::json!({ "unexpected_state_field": "In Progress" }),
			"Invalid `issue.transition` arguments: missing field `state`",
		),
		(
			serde_json::json!({
				"state": "In Progress",
				"unexpected_state_field": "In Progress",
			}),
			"Invalid `issue.transition` arguments: unknown field `unexpected_state_field`",
		),
	];

	for (arguments, message) in cases {
		let response =
			DynamicToolHandler::handle_call(&bridge, ISSUE_TRANSITION_TOOL_NAME, arguments);

		assert!(!response.success);
		assert_eq!(
			response.content_items,
			vec![DynamicToolContentItem::InputText { text: String::from(message) }]
		);
	}

	assert!(tracker.state_updates.borrow().is_empty());
}

#[test]
fn rejects_success_transition_regardless_of_other_workflow_state_membership() {
	for workflow in [
		tests::sample_workflow_with_startable_states(&["Todo", "In Review"]),
		tests::sample_workflow_with_tracker_states(
			&["Todo"],
			"In Progress",
			"In Review",
			"In Review",
		),
		tests::sample_workflow_with_tracker_states(&["Todo"], "In Review", "In Review", "Todo"),
	] {
		let tracker = FakeTracker::new();
		let issue = tests::sample_issue();

		assert_success_transition_requires_review_handoff(workflow, &tracker, &issue);
	}
}

fn assert_success_transition_requires_review_handoff(
	workflow: WorkflowDocument,
	tracker: &FakeTracker,
	issue: &TrackerIssue,
) {
	let bridge = TrackerToolBridge::new(tracker, issue, &workflow);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TRANSITION_TOOL_NAME,
		serde_json::json!({ "state": "In Review" }),
	);

	assert!(!response.success);
	assert!(tracker.state_updates.borrow().is_empty());
	assert_eq!(
		response.content_items,
		vec![DynamicToolContentItem::InputText {
			text: String::from(
				"State `In Review` requires `issue_review_handoff` after the branch is pushed and a reviewable PR exists."
			),
		}]
	);
}
