use crate::agent::tracker_tool_bridge::{
	DynamicToolHandler, ISSUE_LABEL_ADD_TOOL_NAME, ISSUE_TRANSITION_TOOL_NAME, TrackerToolBridge,
	tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker},
};

#[test]
fn rejects_blocking_writes_for_stale_active() {
	for (tool_name, args) in [
		(ISSUE_LABEL_ADD_TOOL_NAME, serde_json::json!({ "label": "decodex:manual-only" })),
		(ISSUE_TRANSITION_TOOL_NAME, serde_json::json!({ "state": "Todo" })),
	] {
		let active_issue = tests::sample_in_progress_issue();
		let tracker = FakeTracker::with_refresh_snapshots(vec![vec![active_issue]]);
		let issue = tests::sample_in_progress_issue();
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

		let error = DynamicToolHandler::classify_turn_completion(
			&bridge,
			"The run started active, so a stale active reread must not clear a local stop write.",
		)
		.expect_err("active-start lanes must keep local stop writes blocking");

		assert!(error.to_string().contains("without recording a terminal path"));
		assert!(error.to_string().contains(tool_name));
	}
}
