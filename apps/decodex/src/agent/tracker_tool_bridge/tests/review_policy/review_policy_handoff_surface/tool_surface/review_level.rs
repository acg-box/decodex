use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolContentItem, DynamicToolHandler, FakeLocalRepoInspector,
	FakePullRequestInspector, FakeTracker, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
	ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ReviewLevel, TempDir,
	TrackerToolBridge,
};

#[test]
fn review_checkpoint_tool_surface_respects_review_level() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let review_issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let mut review_context = tests::sample_review_context_in(temp_dir.path());
	let repair_context = tests::sample_review_repair_context_in(
		temp_dir.path(),
		"https://github.com/hack-ink/decodex/pull/242",
	);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);
	let repair_bridge = TrackerToolBridge::with_review_repair_for_test(
		&tracker,
		&review_issue,
		&workflow,
		repair_context,
		&inspector,
		&local_repo_inspector,
	);
	let tool_names = DynamicToolHandler::tool_specs(&bridge)
		.into_iter()
		.map(|tool| tool.name)
		.collect::<Vec<_>>();
	let repair_tool_names = DynamicToolHandler::tool_specs(&repair_bridge)
		.into_iter()
		.map(|tool| tool.name)
		.collect::<Vec<_>>();

	assert!(!tool_names.contains(&String::from(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME)));
	assert!(tool_names.contains(&String::from(ISSUE_REVIEW_HANDOFF_TOOL_NAME)));
	assert!(!repair_tool_names.contains(&String::from(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME)));
	assert!(repair_tool_names.contains(&String::from(ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME)));

	review_context.review_level = ReviewLevel::Off;

	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context,
		&inspector,
		&local_repo_inspector,
	);
	let checkpoint_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"status": "clean",
			"head_sha": "08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
			"evidence": []
		}),
	);

	assert!(!checkpoint_response.success);
	assert!(matches!(
		checkpoint_response.content_items.as_slice(),
		[DynamicToolContentItem::InputText{ text }]
			if text.contains("[codex].review = \"off\"")
	));
}
