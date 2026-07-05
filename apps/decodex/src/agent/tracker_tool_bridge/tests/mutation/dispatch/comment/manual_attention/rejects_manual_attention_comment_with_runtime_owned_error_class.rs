use crate::agent::tracker_tool_bridge::{
	DynamicToolContentItem, DynamicToolHandler, ISSUE_COMMENT_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME,
	TrackerToolBridge,
	tests::{self, FakeLocalRepoInspector, FakePullRequestInspector},
};

#[test]
fn rejects_manual_attention_comment_with_runtime_owned_error_class() {
	for error_class in [
		"retryable_execution_failure",
		"repo_gate_verify_failed",
		"repo_gate_baseline_failed",
		"repo_gate_preexisting_baseline_failed",
		"repo_gate_global_baseline_failed",
		"repository_wide_docs_okf_check_failed",
		"pre_existing_docs_gate_failed",
		"repo_gate_git_lock_contention",
		"stalled_run_detected",
		"app_server_plugin_list_timeout",
		"app_server_turn_failed",
		"app_server_turn_missing_error_payload",
		"app_server_usage_limit_exceeded",
		"app_server_dynamic_tool_failed",
		"phase_goal_terminal_path_missing",
	] {
		let issue = tests::sample_issue();
		let tracker = tests::tracker_with_current_issue_snapshot(&issue);
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
		let label_response = DynamicToolHandler::handle_call(
			&bridge,
			ISSUE_LABEL_ADD_TOOL_NAME,
			serde_json::json!({ "label": "decodex:needs-attention" }),
		);
		let mut args = tests::manual_attention_comment_args();

		args["error_class"] = serde_json::json!(error_class);

		let response = DynamicToolHandler::handle_call(&bridge, ISSUE_COMMENT_TOOL_NAME, args);

		assert!(label_response.success);
		assert!(
			!response.success,
			"manual attention should reject runtime-owned class {error_class}"
		);
		assert!(tracker.comments.borrow().is_empty());
		assert!(
			tracker.label_additions.borrow().is_empty(),
			"runtime-owned class {error_class} must not leave a needs-attention label"
		);
		assert!(matches!(
			response.content_items.as_slice(),
			[DynamicToolContentItem::InputText { text }]
				if text.contains("cannot use runtime-owned error class")
					&& text.contains(error_class)
		));
	}
}
