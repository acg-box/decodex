use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolHandler, FakeTracker, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
	ISSUE_TERMINAL_FINALIZE_TOOL_NAME, TempDir, TrackerToolBridge, review_policy,
};

#[test]
fn review_repair_apply_preserves_existing_authority_when_comment_write_fails() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::with_comment_error("tracker comment write failed");
	let issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/242";
	let (inspector, local_repo_inspector) =
		review_policy::sample_review_repair_apply_inspectors(pr_url);
	let review_context = tests::sample_review_repair_context_in(temp_dir.path(), pr_url);
	let bridge = TrackerToolBridge::with_review_repair_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);
	let seed_context = tests::sample_review_repair_context_in(temp_dir.path(), pr_url);

	review_policy::seed_review_repair_apply_state(
		tests::bridge_state_store(&bridge),
		&seed_context,
		&issue.id,
		pr_url,
		2,
	);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
		serde_json::json!({
			"pr_url": pr_url,
			"summary": "Addressed the requested review changes."
		}),
	);
	let finalize_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		serde_json::json!({ "path": "review_repair" }),
	);

	assert!(response.success);
	assert!(finalize_response.success);

	let error = bridge
		.apply_review_repair()
		.expect_err("comment write failures must preserve the original lifecycle authority");

	assert!(error.to_string().contains("tracker comment write failed"));
	assert!(tracker.comments.borrow().is_empty());

	let marker = tests::persisted_review_lifecycle_handoff_fixture(&bridge, &issue, &seed_context);

	assert_eq!(marker.pr_url(), pr_url);
	assert_eq!(marker.pr_head_oid(), "08a20f7dfb9526e7421a5f095b1c6adec84e52d6");

	let transition_fixture = tests::persisted_review_lifecycle_transition_fixture(
		&bridge,
		&issue,
		&seed_context,
		&marker,
	);

	assert_eq!(transition_fixture.phase(), "repair_required");
	assert_eq!(transition_fixture.head_sha(), "08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	assert_eq!(transition_fixture.external_round_count(), 2);
}
