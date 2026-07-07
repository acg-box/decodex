use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolHandler, FakeTracker, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
	ISSUE_TERMINAL_FINALIZE_TOOL_NAME, TempDir, TrackerToolBridge, review_policy,
};

#[test]
fn review_repair_apply_does_not_reset_external_round_budget_after_fourth_round() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
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

	review_policy::seed_review_repair_apply_state(
		tests::bridge_state_store(&bridge),
		&review_context,
		&issue.id,
		pr_url,
		4,
	);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
		serde_json::json!({
			"pr_url": pr_url,
			"summary": "Addressed the requested review changes."
		}),
	);

	tests::seed_docs_impact_checkpoint(
		tests::bridge_state_store(&bridge),
		&review_context,
		&issue.id,
		"review_repair",
		"18a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);

	let finalize_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		serde_json::json!({ "path": "review_repair" }),
	);

	assert!(response.success);
	assert!(finalize_response.success);

	DynamicToolHandler::validate_turn_completion(&bridge, "done")
		.expect("review repair completion should allow the turn to complete");

	bridge.apply_review_repair().expect("review repair should apply");

	let marker =
		tests::persisted_review_lifecycle_handoff_fixture(&bridge, &issue, &review_context);
	let transition_fixture = tests::persisted_review_lifecycle_transition_fixture(
		&bridge,
		&issue,
		&review_context,
		&marker,
	);

	assert_eq!(transition_fixture.phase(), "request_pending");
	assert_eq!(transition_fixture.external_round_count(), 4);
}
