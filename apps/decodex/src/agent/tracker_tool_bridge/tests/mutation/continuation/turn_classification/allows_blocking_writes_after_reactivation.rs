use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolHandler, ISSUE_LABEL_ADD_TOOL_NAME, ISSUE_TRANSITION_TOOL_NAME,
		TrackerToolBridge, TurnCompletionStatus,
		tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker},
	},
	tracker::TrackerState,
};

#[test]
fn allows_blocking_writes_after_reactivation() {
	for (tool_name, args) in [
		(ISSUE_LABEL_ADD_TOOL_NAME, serde_json::json!({ "label": "decodex:manual-only" })),
		(ISSUE_TRANSITION_TOOL_NAME, serde_json::json!({ "state": "Todo" })),
	] {
		let mut reactivated_issue = tests::sample_issue();

		reactivated_issue.state =
			TrackerState { id: String::from("state-progress"), name: String::from("In Progress") };

		let tracker = FakeTracker::with_refresh_snapshots(vec![
			vec![reactivated_issue.clone()],
			vec![reactivated_issue],
		]);
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
		let response = DynamicToolHandler::handle_call(&bridge, tool_name, args);

		assert!(response.success);
		assert_eq!(
			DynamicToolHandler::classify_turn_completion(
				&bridge,
				"The issue was reactivated before turn completion, so the stale stop write must not block continuation."
			)
			.expect("startable-start lanes should allow continuation after reactivation"),
			TurnCompletionStatus::Continue
		);
	}
}
