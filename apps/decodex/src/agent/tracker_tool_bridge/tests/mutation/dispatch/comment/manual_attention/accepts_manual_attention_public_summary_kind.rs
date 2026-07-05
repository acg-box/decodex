use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolHandler, ISSUE_COMMENT_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME, TrackerToolBridge,
		tests::{self, FakeLocalRepoInspector, FakePullRequestInspector},
	},
	tracker::records,
};

#[test]
fn accepts_manual_attention_public_summary_kind() {
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

	assert!(label_response.success);
	assert!(
		tracker.label_additions.borrow().is_empty(),
		"manual-attention label intent must not mutate Linear before comment validation"
	);

	let comment_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_COMMENT_TOOL_NAME,
		tests::manual_attention_comment_args(),
	);

	assert!(comment_response.success);

	let comments = tracker.comments.borrow();
	let comment = comments.first().expect("manual attention summary should write");
	let record = records::parse_linear_execution_event_record(comment)
		.expect("manual attention summary should include a ledger record");

	assert_eq!(tracker.label_additions.borrow().as_slice(), [vec![String::from("label-needs")]]);
	assert_eq!(comments.len(), 1);
	assert!(comment.contains("decodex run needs manual attention"));
	assert!(comment.contains("- worktree_path: `.worktrees/PUB-618`"));
	assert!(comment.contains("- failed_command: cargo make test"));
	assert!(comment.contains("- raw_error: repo gate failed with public test output"));
	assert_eq!(record.event_type, "needs_attention");
	assert_eq!(record.error_class.as_deref(), Some("operator_decision_required"));
	assert_eq!(record.failed_command.as_deref(), Some("cargo make test"));
	assert_eq!(record.raw_error.as_deref(), Some("repo gate failed with public test output"));
}
