use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolContentItem, DynamicToolHandler, FakeLocalRepoInspector,
	FakePullRequestInspector, FakeTracker, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
	ReviewCheckpointArtifactLookup, TempDir, TrackerToolBridge, review_policy,
};

#[test]
fn review_checkpoint_rejects_review_blocking_local_changes() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector =
		FakeLocalRepoInspector::new(vec![Ok(review_policy::sample_dirty_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "clean",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::handoff_review_contract_json(),
			"review_cost_control": review_policy::compact_review_cost_control_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["review tried to bind a dirty worktree"]
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("requires a clean committed lane HEAD")
				&& text.contains("M apps/decodex/src/agent/tracker_tool_bridge/tools.rs")
				&& text.contains("?? apps/decodex/src/agent/new_review_surface.rs")
	));
	assert!(
		tests::bridge_state_store(&bridge)
			.review_checkpoint_artifact(ReviewCheckpointArtifactLookup {
				project_id: &review_context.service_id,
				issue_id: &issue.id,
				phase: "handoff",
				review_level: review_context.review_level.as_str(),
				head_sha: &tests::sample_local_repo().head_oid,
			})
			.expect("artifact lookup should succeed")
			.is_none(),
		"dirty checkpoint attempts must not persist reusable review evidence"
	);
}
