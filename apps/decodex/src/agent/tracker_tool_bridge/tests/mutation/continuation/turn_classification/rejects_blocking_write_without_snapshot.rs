use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolHandler, ISSUE_LABEL_ADD_TOOL_NAME, TrackerToolBridge,
		tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker},
	},
	tracker::TrackerLabel,
};

#[test]
fn rejects_blocking_write_without_snapshot() {
	let mut opted_out_issue = tests::sample_issue();

	opted_out_issue.labels.push(TrackerLabel {
		id: String::from("label-manual"),
		name: String::from("decodex:manual-only"),
	});

	let tracker = FakeTracker::with_refresh_snapshots(vec![vec![opted_out_issue], Vec::new()]);
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
		serde_json::json!({ "label": "decodex:manual-only" }),
	);

	assert!(response.success);

	let error = DynamicToolHandler::classify_turn_completion(
		&bridge,
		"The lane recorded a continuation-blocking tracker write without a terminal path.",
	)
	.expect_err("missing refresh snapshots must not allow a clean continuation boundary");

	assert!(error.to_string().contains("without recording a terminal path"));
	assert!(error.to_string().contains(ISSUE_LABEL_ADD_TOOL_NAME));
}
