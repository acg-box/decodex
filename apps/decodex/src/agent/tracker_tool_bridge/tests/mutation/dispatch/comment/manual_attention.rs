use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolContentItem, DynamicToolHandler, ISSUE_COMMENT_TOOL_NAME,
		ISSUE_LABEL_ADD_TOOL_NAME, TrackerToolBridge,
		tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker},
	},
	tracker::records,
};

#[test]
fn accepts_manual_attention_public_summary_kind() {
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

	assert!(label_response.success);
	assert!(
		tracker.label_additions.borrow().is_empty(),
		"manual-attention label intent must not mutate Linear before comment validation"
	);

	let comment_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_COMMENT_TOOL_NAME,
		tests::manual_attention_comment_args(),
	);

	assert!(comment_response.success);

	let comments = tracker.comments.borrow();
	let comment = comments.first().expect("manual attention summary should write");
	let record = records::parse_linear_execution_event_record(comment)
		.expect("manual attention summary should include a ledger record");

	assert_eq!(tracker.label_additions.borrow().as_slice(), [vec![String::from("label-needs")]]);
	assert_eq!(comments.len(), 1);
	assert!(comment.contains("decodex run needs manual attention"));
	assert!(comment.contains("- worktree_path: `.worktrees/PUB-618`"));
	assert!(comment.contains("- failed_command: cargo make test"));
	assert!(comment.contains("- raw_error: repo gate failed with public test output"));
	assert_eq!(record.event_type, "needs_attention");
	assert_eq!(record.error_class.as_deref(), Some("operator_decision_required"));
	assert_eq!(record.failed_command.as_deref(), Some("cargo make test"));
	assert_eq!(record.raw_error.as_deref(), Some("repo gate failed with public test output"));
}

#[test]
fn rejects_manual_attention_comments_with_private_or_unsupported_fields() {
	for (field_name, value, expected_error) in [
		(
			"failed_command",
			"cargo test --manifest-path /Users/example/repo/Cargo.toml",
			"`failed_command` must be public/team-visible text; host-local paths are not allowed.",
		),
		(
			"raw_error",
			"Missing GITHUB_PAT_Y.",
			"`raw_error` must be public/team-visible text; credential-like names are not allowed.",
		),
		(
			"next_action",
			"inspect /Users/example/repo and recover manually",
			"`next_action` must be public/team-visible text; host-local paths are not allowed.",
		),
		(
			"blockers",
			"local checkout at /Users/example/repo blocked validation",
			"`blockers` must be public/team-visible text; host-local paths are not allowed.",
		),
		(
			"evidence",
			"selected account user@example.com failed",
			"`evidence` must be public/team-visible text; private identity details are not allowed.",
		),
		(
			"summary",
			"Missing GITHUB_PAT_Y blocked automation.",
			"`summary` must be public/team-visible text; credential-like names are not allowed.",
		),
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

		args[field_name] = if matches!(field_name, "blockers" | "evidence") {
			serde_json::json!([value])
		} else {
			serde_json::json!(value)
		};

		let response = DynamicToolHandler::handle_call(&bridge, ISSUE_COMMENT_TOOL_NAME, args);

		assert!(label_response.success);
		assert!(!response.success, "comment should be rejected for {field_name}");
		assert!(tracker.comments.borrow().is_empty());
		assert!(
			tracker.label_additions.borrow().is_empty(),
			"rejected manual-attention comment must not leave a needs-attention label"
		);
		assert_eq!(
			response.content_items,
			vec![DynamicToolContentItem::InputText { text: String::from(expected_error) }]
		);
	}
}

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

#[test]
fn rejects_unsupported_issue_comment_kinds_without_mutation() {
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
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_COMMENT_TOOL_NAME,
		serde_json::json!({
			"kind": "status_note",
			"error_class": "operator_decision_required",
			"next_action": "resolve manually",
			"blockers": ["operator decision is required"],
			"evidence": ["agent attempted unsupported comment kind"]
		}),
	);

	assert!(label_response.success);
	assert!(!response.success);
	assert!(tracker.comments.borrow().is_empty());
	assert!(
		tracker.label_additions.borrow().is_empty(),
		"unsupported comment kind must not leave a needs-attention label"
	);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("Unsupported `issue_comment` kind `status_note`")
	));
}

#[test]
fn rejects_manual_attention_comment_before_needs_attention_label() {
	let issue = tests::sample_issue();
	let tracker = FakeTracker::new();
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
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_COMMENT_TOOL_NAME,
		tests::manual_attention_comment_args(),
	);

	assert!(!response.success);
	assert!(tracker.comments.borrow().is_empty());
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("requires a successful `issue_label_add` call")
	));
}

#[test]
fn rejects_manual_attention_comment_with_non_public_error_class_shape() {
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

	args["error_class"] = serde_json::json!("Missing-GITHUB_PAT_Y");

	let response = DynamicToolHandler::handle_call(&bridge, ISSUE_COMMENT_TOOL_NAME, args);

	assert!(label_response.success);
	assert!(!response.success);
	assert!(tracker.comments.borrow().is_empty());
	assert!(
		tracker.label_additions.borrow().is_empty(),
		"invalid error_class shape must not leave a needs-attention label"
	);
	assert_eq!(
		response.content_items,
		vec![DynamicToolContentItem::InputText {
			text: String::from("`error_class` must be a public snake_case identifier.")
		}]
	);
}
