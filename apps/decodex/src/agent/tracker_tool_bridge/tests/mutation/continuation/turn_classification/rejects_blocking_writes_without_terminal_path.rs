use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolHandler, ISSUE_LABEL_ADD_TOOL_NAME, ISSUE_TRANSITION_TOOL_NAME,
		TrackerToolBridge,
		tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker},
	},
	tracker::{TrackerLabel, TrackerState},
};

#[test]
fn rejects_blocking_writes_without_terminal_path() {
	for (tool_name, args) in [
		(ISSUE_LABEL_ADD_TOOL_NAME, serde_json::json!({ "label": "decodex:manual-only" })),
		(ISSUE_TRANSITION_TOOL_NAME, serde_json::json!({ "state": "Todo" })),
	] {
		let mut refreshed_issue = tests::sample_issue();

		if tool_name == ISSUE_LABEL_ADD_TOOL_NAME {
			refreshed_issue.labels.push(TrackerLabel {
				id: String::from("label-manual"),
				name: String::from("decodex:manual-only"),
			});
		} else {
			refreshed_issue.state =
				TrackerState { id: String::from("state-todo"), name: String::from("Todo") };
		}

		let tracker = FakeTracker::with_refresh_snapshots(vec![vec![refreshed_issue]]);
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

		let error = DynamicToolHandler::classify_turn_completion(
			&bridge,
			"The lane recorded a continuation-blocking tracker write without a terminal path.",
		)
		.expect_err("continuation-blocking writes must not exit via a clean boundary");

		assert!(error.to_string().contains("without recording a terminal path"));
		assert!(error.to_string().contains(tool_name));
	}
}
