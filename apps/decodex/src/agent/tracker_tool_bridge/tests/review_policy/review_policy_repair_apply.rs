use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolHandler, FakeTracker, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
	ISSUE_TERMINAL_FINALIZE_TOOL_NAME, TempDir, TrackerToolBridge, review_policy,
};

#[test]
fn review_repair_apply_persists_updated_handoff_marker_without_tracker_transition() {
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

	tests::assert_review_policy_checkpoint_cleared(&bridge, &issue, &review_context);
	DynamicToolHandler::validate_turn_completion(&bridge, "done")
		.expect("review repair completion should allow the turn to complete");

	bridge.apply_review_repair().expect("review repair should apply");

	assert!(tracker.state_updates.borrow().is_empty());

	let comments = tracker.comments.borrow();

	assert_eq!(comments.len(), 1);
	assert!(comments[0].contains("fresh review"));
	assert!(comments[0].contains("- pr_url: `https://github.com/hack-ink/decodex/pull/242`"));

	let marker = tests::persisted_review_handoff_marker(&bridge, &issue, &review_context);

	assert_eq!(marker.pr_url(), pr_url);
	assert_eq!(marker.pr_head_oid(), "18a20f7dfb9526e7421a5f095b1c6adec84e52d6");

	let orchestration_marker =
		tests::persisted_review_orchestration_marker(&bridge, &issue, &review_context, &marker);

	assert_eq!(orchestration_marker.phase(), "request_pending");
	assert_eq!(orchestration_marker.head_sha(), "18a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	assert_eq!(orchestration_marker.external_round_count(), 2);
}

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

	let marker = tests::persisted_review_handoff_marker(&bridge, &issue, &review_context);
	let orchestration_marker =
		tests::persisted_review_orchestration_marker(&bridge, &issue, &review_context, &marker);

	assert_eq!(orchestration_marker.phase(), "request_pending");
	assert_eq!(orchestration_marker.external_round_count(), 4);
}

#[test]
fn review_repair_apply_preserves_existing_markers_when_comment_write_fails() {
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

	let error = bridge
		.apply_review_repair()
		.expect_err("comment write failures must preserve the original handoff marker");

	assert!(error.to_string().contains("tracker comment write failed"));
	assert!(tracker.comments.borrow().is_empty());

	let marker = tests::persisted_review_handoff_marker(&bridge, &issue, &seed_context);

	assert_eq!(marker.pr_url(), pr_url);
	assert_eq!(marker.pr_head_oid(), "08a20f7dfb9526e7421a5f095b1c6adec84e52d6");

	let orchestration_marker =
		tests::persisted_review_orchestration_marker(&bridge, &issue, &seed_context, &marker);

	assert_eq!(orchestration_marker.phase(), "repair_required");
	assert_eq!(orchestration_marker.head_sha(), "08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	assert_eq!(orchestration_marker.external_round_count(), 2);
}
