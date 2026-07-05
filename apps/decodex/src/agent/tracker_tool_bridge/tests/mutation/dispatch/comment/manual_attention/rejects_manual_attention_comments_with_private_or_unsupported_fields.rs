use crate::agent::tracker_tool_bridge::{
	DynamicToolContentItem, DynamicToolHandler, ISSUE_COMMENT_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME,
	TrackerToolBridge,
	tests::{self, FakeLocalRepoInspector, FakePullRequestInspector},
};

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
