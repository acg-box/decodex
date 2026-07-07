use tempfile::TempDir;

use crate::agent::tracker_tool_bridge::{
	DynamicToolHandler, ISSUE_REVIEW_HANDOFF_TOOL_NAME, PullRequestDetails,
	ReviewHandoffWritebackFailed, TrackerToolBridge,
	tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker, TEST_SERVICE_ID},
};

#[test]
fn review_handoff_validation_failure_reports_recoverable_writeback_with_pr_url() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let pull_request = PullRequestDetails {
		head_ref_name: String::from("x/decodex-pub-618"),
		head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		head_repository_name: String::from("decodex"),
		head_repository_owner: String::from("hack-ink"),
		is_draft: false,
		state: String::from("OPEN"),
		base_ref_name: String::from("main"),
		url: String::from("https://github.com/hack-ink/decodex/pull/152"),
	};
	let inspector =
		FakePullRequestInspector::new(vec![Ok(pull_request.clone()), Ok(pull_request.clone())]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let mut review_context = tests::sample_review_context_in(temp_dir.path());

	review_context.worktree_path = String::from("/Users/example/repo/.worktrees/PUB-618");

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
			"pr_url": pull_request.url,
			"summary": "Ready for review."
		}),
	);

	assert!(response.success);

	let error = bridge
		.apply_review_handoff()
		.expect_err("remaining public writeback validation failures should be recoverable");
	let writeback_error = error
		.downcast_ref::<ReviewHandoffWritebackFailed>()
		.expect("validation failure should use dedicated writeback error type");

	assert_eq!(writeback_error.pr_url, "https://github.com/hack-ink/decodex/pull/152");
	assert_eq!(writeback_error.success_state, "In Review");
	assert!(writeback_error.source.contains("failed to prepare the tracker review handoff record"));
	assert!(
		writeback_error.source.contains("public/team-visible")
			|| writeback_error.source.contains("repository-relative"),
		"unexpected writeback source: {}",
		writeback_error.source
	);
	assert!(tracker.comments.borrow().is_empty());
	assert!(tracker.state_updates.borrow().is_empty());
	assert!(
		tests::bridge_state_store(&bridge)
			.review_lifecycle_handoff_fixture(TEST_SERVICE_ID, &issue.id, "x/decodex-pub-618")
			.expect("runtime lifecycle authority read should succeed")
			.is_none()
	);
}
