use tempfile::TempDir;

use crate::agent::tracker_tool_bridge::{
	DynamicToolHandler, ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
	PullRequestDetails, ReviewHandoffWritebackFailed, TrackerToolBridge,
	tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker, TEST_SERVICE_ID},
};

#[test]
fn reports_review_handoff_writeback_failure_when_tracker_comment_write_fails() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::with_comment_error("tracker comment write failed");
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
			url: String::from("https://github.com/hack-ink/decodex/pull/50"),
		}),
		Ok(PullRequestDetails {
			head_ref_name: String::from("x/decodex-pub-618"),
			head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_repository_name: String::from("decodex"),
			head_repository_owner: String::from("hack-ink"),
			is_draft: false,
			state: String::from("OPEN"),
			base_ref_name: String::from("main"),
			url: String::from("https://github.com/hack-ink/decodex/pull/50"),
		}),
		Ok(PullRequestDetails {
			head_ref_name: String::from("x/decodex-pub-618"),
			head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_repository_name: String::from("decodex"),
			head_repository_owner: String::from("hack-ink"),
			is_draft: false,
			state: String::from("OPEN"),
			base_ref_name: String::from("main"),
			url: String::from("https://github.com/hack-ink/decodex/pull/50"),
		}),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
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
			"pr_url": "https://github.com/hack-ink/decodex/pull/50",
			"summary": "Ready for review."
		}),
	);

	assert!(response.success);

	tests::bridge_state_store(&bridge)
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			&review_context.run_id,
			review_context.attempt_number,
			"progress_checkpoint",
			serde_json::json!({
				"phase": "ready_for_review",
				"docs_impact": "none",
				"head_sha": tests::sample_local_repo().head_oid,
				"focus": "Finalize review handoff.",
				"next_action": "Record terminal finalize.",
				"blockers": [],
				"evidence": ["Review handoff recorded."]
			}),
		)
		.expect("private progress checkpoint should seed");

	let finalize_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		serde_json::json!({ "path": "review_handoff" }),
	);

	assert!(finalize_response.success);

	let error = bridge.apply_review_handoff().expect_err(
		"comment write failures after state transition must surface as partial writeback",
	);
	let writeback_error = error
		.downcast_ref::<ReviewHandoffWritebackFailed>()
		.expect("partial writeback should use dedicated error type");

	assert_eq!(writeback_error.issue_identifier, "DEC-1");
	assert_eq!(writeback_error.run_id, "pub-618-attempt-2-123");
	assert_eq!(writeback_error.success_state, "In Review");
	assert!(writeback_error.source.contains("failed to persist the tracker review handoff record"));
	assert!(writeback_error.source.contains("tracker comment write failed"));
	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.comments.borrow().is_empty());
	assert_eq!(
		tests::bridge_state_store(&bridge)
			.review_lifecycle_handoff_fixture(TEST_SERVICE_ID, &issue.id, "x/decodex-pub-618")
			.expect("runtime lifecycle authority read should succeed")
			.expect("tracker writeback failure should keep durable lifecycle authority")
			.pr_url(),
		"https://github.com/hack-ink/decodex/pull/50"
	);
}
