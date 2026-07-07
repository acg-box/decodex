use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolHandler, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker,
	ISSUE_COMMENT_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
	ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
	ISSUE_TERMINAL_FINALIZE_TOOL_NAME, ISSUE_TRANSITION_TOOL_NAME, TempDir, TrackerToolBridge,
};

#[test]
fn review_repair_tool_surface_excludes_issue_transition() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/242";
	let temp_dir = TempDir::new().expect("tempdir should create");
	let inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let bridge = TrackerToolBridge::with_review_repair_for_test(
		&tracker,
		&issue,
		&workflow,
		tests::sample_review_repair_context_in(temp_dir.path(), pr_url),
		&inspector,
		&local_repo_inspector,
	);
	let tool_names = DynamicToolHandler::tool_specs(&bridge)
		.into_iter()
		.map(|tool| tool.name)
		.collect::<Vec<_>>();

	assert!(!tool_names.contains(&String::from(ISSUE_TRANSITION_TOOL_NAME)));
	assert!(tool_names.contains(&String::from(ISSUE_COMMENT_TOOL_NAME)));
	assert!(tool_names.contains(&String::from(ISSUE_LABEL_ADD_TOOL_NAME)));
	assert!(tool_names.contains(&String::from(ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME)));
	assert!(!tool_names.contains(&String::from(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME)));
	assert!(tool_names.contains(&String::from(ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME)));
	assert!(tool_names.contains(&String::from(ISSUE_TERMINAL_FINALIZE_TOOL_NAME)));
}
