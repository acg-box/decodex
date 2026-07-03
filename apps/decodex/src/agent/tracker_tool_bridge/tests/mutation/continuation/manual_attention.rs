use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolHandler, ISSUE_COMMENT_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME,
		RunCompletionDisposition, TrackerToolBridge,
		tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker},
	},
	tracker::records,
};

#[test]
fn completion_disposition_allows_manual_attention_exit_without_review_handoff() {
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
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_LABEL_ADD_TOOL_NAME,
		serde_json::json!({ "label": "decodex:needs-attention" }),
	);
	let comment_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_COMMENT_TOOL_NAME,
		tests::manual_attention_comment_args(),
	);

	assert!(response.success);
	assert!(comment_response.success);

	let comment =
		tracker.comments.borrow().first().expect("manual attention comment should write").clone();
	let record = records::parse_linear_execution_event_record(&comment)
		.expect("manual attention comment should include a ledger record");

	assert_eq!(record.event_type, "needs_attention");
	assert_eq!(record.error_class.as_deref(), Some("operator_decision_required"));
	assert_eq!(record.terminal_path.as_deref(), Some("manual_attention"));
	assert_eq!(
		bridge.completion_disposition().expect("manual attention should be accepted"),
		RunCompletionDisposition::ManualAttention
	);
}

#[test]
fn manual_attention_requires_explanatory_comment() {
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
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_LABEL_ADD_TOOL_NAME,
		serde_json::json!({ "label": "decodex:needs-attention" }),
	);

	assert!(response.success);
	assert!(
		tracker.label_additions.borrow().is_empty(),
		"manual-attention intent alone must not mutate Linear"
	);

	let error = bridge
		.completion_disposition()
		.expect_err("manual attention must require an explanatory comment");

	assert!(error.to_string().contains("never recorded the required explanatory comment"));
}

#[test]
fn failed_needs_attention_label_update_does_not_record_manual_attention() {
	let tracker = FakeTracker::with_label_update_error("tracker labels unavailable");
	let issue = tests::sample_issue();
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
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_LABEL_ADD_TOOL_NAME,
		serde_json::json!({ "label": "decodex:needs-attention" }),
	);
	let comment_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_COMMENT_TOOL_NAME,
		tests::manual_attention_comment_args(),
	);

	assert!(response.success);
	assert!(!comment_response.success);
	assert!(tracker.label_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.comments.borrow().is_empty());

	let error = bridge
		.completion_disposition()
		.expect_err("failed label writes must not count as manual attention");

	assert!(error.to_string().contains("never recorded the required explanatory comment"));
}
