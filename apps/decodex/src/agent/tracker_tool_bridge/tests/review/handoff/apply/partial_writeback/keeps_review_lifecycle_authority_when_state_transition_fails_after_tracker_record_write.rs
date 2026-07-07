use tempfile::TempDir;

use crate::agent::tracker_tool_bridge::{
	DynamicToolHandler, ISSUE_REVIEW_HANDOFF_TOOL_NAME, PullRequestDetails, TrackerToolBridge,
	tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker, TEST_SERVICE_ID},
};

#[test]
fn keeps_review_lifecycle_authority_when_state_transition_fails_after_tracker_record_write() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::with_state_update_error("tracker state write failed");
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(vec![
		Ok(PullRequestDetails {
			head_ref_name: String::from("x/decodex-pub-618"),
			head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_repository_name: String::from("decodex"),
			head_repository_owner: String::from("hack-ink"),
			is_draft: false,
			state: String::from("OPEN"),
			base_ref_name: String::from("main"),
			url: String::from("https://github.com/hack-ink/decodex/pull/49"),
		}),
		Ok(PullRequestDetails {
			head_ref_name: String::from("x/decodex-pub-618"),
			head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_repository_name: String::from("decodex"),
			head_repository_owner: String::from("hack-ink"),
			is_draft: false,
			state: String::from("OPEN"),
			base_ref_name: String::from("main"),
			url: String::from("https://github.com/hack-ink/decodex/pull/49"),
		}),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let review_context = tests::sample_review_context_in(temp_dir.path());
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
			"pr_url": "https://github.com/hack-ink/decodex/pull/49",
			"summary": "Ready for review."
		}),
	);

	assert!(response.success);

	let error = bridge.apply_review_handoff().expect_err(
		"state transition failures must surface after the tracker handoff record write",
	);

	assert!(error.to_string().contains("tracker state write failed"));
	assert_eq!(tracker.comments.borrow().len(), 1);
	assert!(tracker.state_updates.borrow().is_empty());
	assert_eq!(
		tests::bridge_state_store(&bridge)
			.review_lifecycle_handoff_fixture(TEST_SERVICE_ID, &issue.id, "x/decodex-pub-618")
			.expect("runtime lifecycle authority read should succeed")
			.expect("partial review handoff should keep the retained lifecycle authority")
			.pr_url(),
		"https://github.com/hack-ink/decodex/pull/49"
	);
}
