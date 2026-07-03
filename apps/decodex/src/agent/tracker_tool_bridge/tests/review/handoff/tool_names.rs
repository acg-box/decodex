use crate::agent::tracker_tool_bridge::{
	DynamicToolHandler, TrackerToolBridge,
	tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker},
};

#[test]
fn publishes_protocol_safe_tool_names() {
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
	let tool_specs = DynamicToolHandler::tool_specs(&bridge);

	assert!(!tool_specs.is_empty());
	assert!(tool_specs.into_iter().all(|tool| {
		tool.name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
	}));
}
