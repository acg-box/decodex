use crate::agent::tracker_tool_bridge::{
	DynamicToolHandler, TrackerToolBridge, TurnCompletionStatus,
	tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker},
};

#[test]
fn allows_continuation_without_tracker_action() {
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

	assert_eq!(
		DynamicToolHandler::classify_turn_completion(
			&bridge,
			"Still implementing; no terminal tracker action has been recorded yet."
		)
		.expect("missing terminal action should request continuation"),
		TurnCompletionStatus::Continue
	);
}
