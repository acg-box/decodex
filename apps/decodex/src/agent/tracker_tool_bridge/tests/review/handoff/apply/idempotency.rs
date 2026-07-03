use tempfile::TempDir;

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolHandler, ISSUE_REVIEW_HANDOFF_TOOL_NAME, TrackerToolBridge,
		tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker},
	},
	tracker::{
		TrackerComment,
		records::{self, REVIEW_HANDOFF_RECORD_TYPE, ReviewHandoffRecord},
	},
};

#[test]
fn review_handoff_apply_writes_coarse_comment_without_replaying_existing_records() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let mut pull_request = tests::sample_pull_request();

	pull_request.url = String::from("https://github.com/hack-ink/decodex/pull/47");

	let inspector =
		FakePullRequestInspector::new(vec![Ok(pull_request.clone()), Ok(pull_request.clone())]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let existing_record = records::append_structured_comment_record(
		"Review handoff already persisted.",
		&ReviewHandoffRecord {
			record_type: String::from(REVIEW_HANDOFF_RECORD_TYPE),
			completed_at: String::from("2026-04-12T00:00:00Z"),
			run_id: review_context.run_id.clone(),
			attempt_number: review_context.attempt_number,
			branch_name: review_context.branch_name.clone(),
			pr_url: pull_request.url.clone(),
			target_base_ref_name: pull_request.base_ref_name.clone(),
			pr_head_ref_name: pull_request.head_ref_name.clone(),
			pr_head_oid: pull_request.head_ref_oid.clone(),
			summary: String::from("Ready for review."),
		},
	)
	.expect("review handoff record should serialize");

	tracker.issue_comments.borrow_mut().insert(
		issue.id.clone(),
		vec![TrackerComment {
			body: existing_record,
			created_at: String::from("2026-04-12T00:00:00Z"),
		}],
	);

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

	bridge
		.apply_review_handoff()
		.expect("writeback should persist runtime state and coarse tracker summary");

	assert_eq!(tracker.comments.borrow().len(), 1);
	assert_eq!(tracker.state_updates.borrow().as_slice(), [String::from("state-review")]);
}

#[test]
fn review_handoff_apply_does_not_duplicate_existing_ledger_event() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let pull_request = tests::sample_pull_request();
	let inspector = FakePullRequestInspector::new(vec![
		Ok(pull_request.clone()),
		Ok(pull_request.clone()),
		Ok(pull_request.clone()),
		Ok(pull_request.clone()),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let review_context = tests::sample_review_context_in(temp_dir.path());

	for _ in 0..2 {
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
				"pr_url": pull_request.url.clone(),
				"summary": "Ready for review."
			}),
		);

		assert!(response.success);

		bridge.apply_review_handoff().expect("review handoff should apply");
	}

	assert_eq!(tracker.comments.borrow().len(), 1);

	let record = records::parse_linear_execution_event_record(&tracker.comments.borrow()[0])
		.expect("review handoff should write a Linear execution event");

	assert_eq!(record.event_type, "review_handoff");
	assert_eq!(record.idempotency_key.matches("review_handoff").count(), 1);
}
