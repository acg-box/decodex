use std::process;

use tempfile::TempDir;

use crate::agent::tracker_tool_bridge::{
	DynamicToolContentItem, DynamicToolHandler, ISSUE_REVIEW_HANDOFF_TOOL_NAME, TrackerToolBridge,
	tests::{
		self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker,
		GitHubTokenAssertingPullRequestInspector, TestEnvVarGuard,
	},
};

#[test]
fn review_handoff_inspection_uses_configured_github_token() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let token_env_var = "DECODEX_TEST_REVIEW_HANDOFF_GITHUB_TOKEN";
	let _env_guard = TestEnvVarGuard::set(token_env_var, "configured-review-token");
	let inspector = GitHubTokenAssertingPullRequestInspector {
		expected_token: String::from("configured-review-token"),
		response: tests::sample_pull_request(),
	};
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
	let mut review_context = tests::sample_review_context_in(temp_dir.path());

	review_context.github_token_env_var = Some(String::from(token_env_var));

	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);

	tests::write_clean_review_checkpoint(&bridge, &issue, &review_context);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": "https://github.com/hack-ink/decodex/pull/48",
			"summary": "Ready for review."
		}),
	);

	assert!(response.success);
}

#[test]
fn review_handoff_inspection_rejects_missing_or_blank_github_token() {
	{
		let temp_dir = TempDir::new().expect("tempdir should create");
		let tracker = FakeTracker::new();
		let issue = tests::sample_issue();
		let workflow = tests::sample_workflow();
		let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
		let local_repo_inspector =
			FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
		let mut review_context = tests::sample_review_context_in(temp_dir.path());

		review_context.github_token_env_var = None;

		let bridge = TrackerToolBridge::with_review_handoff_for_test(
			&tracker,
			&issue,
			&workflow,
			review_context.clone(),
			&pull_request_inspector,
			&local_repo_inspector,
		);

		tests::write_clean_review_checkpoint(&bridge, &issue, &review_context);

		let response = DynamicToolHandler::handle_call(
			&bridge,
			ISSUE_REVIEW_HANDOFF_TOOL_NAME,
			serde_json::json!({
				"pr_url": "https://github.com/hack-ink/decodex/pull/48",
				"summary": "Ready for review."
			}),
		);

		assert!(!response.success);
		assert_eq!(
			response.content_items,
			vec![DynamicToolContentItem::InputText {
				text: String::from(
					"`github.token_env_var` must be configured for PR-backed review handoff validation.",
				),
			}]
		);
	}
	{
		let temp_dir = TempDir::new().expect("tempdir should create");
		let tracker = FakeTracker::new();
		let issue = tests::sample_issue();
		let workflow = tests::sample_workflow();
		let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
		let local_repo_inspector =
			FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
		let env_var =
			format!("DECODEX_TEST_BLANK_REVIEW_HANDOFF_GITHUB_TOKEN_ENV_{}", process::id());
		let _env_guard = TestEnvVarGuard::set(&env_var, "");
		let mut review_context = tests::sample_review_context_in(temp_dir.path());

		review_context.github_token_env_var = Some(env_var.clone());

		let bridge = TrackerToolBridge::with_review_handoff_for_test(
			&tracker,
			&issue,
			&workflow,
			review_context.clone(),
			&pull_request_inspector,
			&local_repo_inspector,
		);

		tests::write_clean_review_checkpoint(&bridge, &issue, &review_context);

		let response = DynamicToolHandler::handle_call(
			&bridge,
			ISSUE_REVIEW_HANDOFF_TOOL_NAME,
			serde_json::json!({
				"pr_url": "https://github.com/hack-ink/decodex/pull/48",
				"summary": "Ready for review."
			}),
		);

		assert!(!response.success);
		assert_eq!(
			response.content_items,
			vec![DynamicToolContentItem::InputText {
				text: format!(
					"Environment variable `{env_var}` referenced by `github.token_env_var` must not be blank."
				),
			}]
		);
	}
}
