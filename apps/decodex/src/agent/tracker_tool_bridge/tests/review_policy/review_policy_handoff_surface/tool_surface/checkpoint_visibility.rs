use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolHandler, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker,
	ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, TempDir,
	TrackerToolBridge,
};

#[test]
fn review_checkpoint_tool_surface_is_runtime_owned() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let review_issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let handoff_pr_inspector = FakePullRequestInspector::new(Vec::new());
	let handoff_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let handoff_bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		tests::sample_review_context_in(temp_dir.path()),
		&handoff_pr_inspector,
		&handoff_repo_inspector,
	);
	let repair_pr_inspector = FakePullRequestInspector::new(Vec::new());
	let repair_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let repair_bridge = TrackerToolBridge::with_review_repair_for_test(
		&tracker,
		&review_issue,
		&workflow,
		tests::sample_review_repair_context_in(
			temp_dir.path(),
			"https://github.com/hack-ink/decodex/pull/242",
		),
		&repair_pr_inspector,
		&repair_repo_inspector,
	);
	let closeout_bridge = TrackerToolBridge::with_run_context(
		&tracker,
		&review_issue,
		&workflow,
		tests::sample_closeout_context_in(
			temp_dir.path(),
			"https://github.com/hack-ink/decodex/pull/260",
		),
	);
	let handoff_tools = DynamicToolHandler::tool_specs(&handoff_bridge)
		.into_iter()
		.map(|tool| tool.name)
		.collect::<Vec<_>>();
	let repair_tools = DynamicToolHandler::tool_specs(&repair_bridge)
		.into_iter()
		.map(|tool| tool.name)
		.collect::<Vec<_>>();
	let closeout_tools = DynamicToolHandler::tool_specs(&closeout_bridge)
		.into_iter()
		.map(|tool| tool.name)
		.collect::<Vec<_>>();

	assert!(handoff_tools.contains(&String::from(ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME)));
	assert!(repair_tools.contains(&String::from(ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME)));
	assert!(closeout_tools.contains(&String::from(ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME)));
	assert!(!handoff_tools.contains(&String::from(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME)));
	assert!(!repair_tools.contains(&String::from(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME)));
	assert!(!closeout_tools.contains(&String::from(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME)));
}
