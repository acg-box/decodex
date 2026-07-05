use crate::agent::tracker_tool_bridge::{
	DynamicToolHandler, TrackerToolBridge,
	tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker},
};

#[test]
fn turn_completion_rejects_xy_156_shape_without_terminal_tracker_action() {
	let tracker = FakeTracker::new();
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
	let error = DynamicToolHandler::validate_turn_completion(
		&bridge,
		"Implementation and tests are done, but commit, push, PR, and tracker handoff remain.",
	)
	.expect_err("turn completion should reject missing terminal tracker actions");

	assert!(error.to_string().contains("recorded neither `issue_review_handoff`"));
}
