use tempfile::TempDir;

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolHandler, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME, TrackerToolBridge,
		tests::{
			self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker, TEST_SERVICE_ID,
		},
	},
	tracker::{
		self, TrackerComment, TrackerLabel, TrackerState,
		records::{self, CLOSEOUT_RECORD_TYPE, CloseoutRecord},
	},
};

#[test]
fn closeout_apply_validates_merged_pr_and_completed_issue_state() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let mut completed_issue = tests::sample_review_issue();

	completed_issue.state =
		TrackerState { id: String::from("state-done"), name: String::from("Done") };

	let tracker = FakeTracker::with_refresh_snapshots(vec![
		vec![completed_issue.clone()],
		vec![completed_issue],
	]);
	let issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/260";
	let mut merged_pull_request = tests::sample_pull_request();

	merged_pull_request.url = String::from(pr_url);
	merged_pull_request.state = String::from("MERGED");

	let inspector = FakePullRequestInspector::new(vec![
		Ok(merged_pull_request.clone()),
		Ok(merged_pull_request),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let review_context = tests::sample_closeout_context_in(temp_dir.path(), pr_url);
	let bridge = TrackerToolBridge::with_review_handoff_inspectors(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		Some(TrackerToolBridge::leaked_test_state_store()),
		&inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
		serde_json::json!({
		"pr_url": pr_url,
		"summary": "Merged the approved lane and finished closeout."
		}),
	);

	tests::seed_docs_impact_checkpoint(
		tests::bridge_state_store(&bridge),
		&review_context,
		&issue.id,
		"closeout",
		&tests::sample_local_repo().head_oid,
	);

	let finalize_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		serde_json::json!({ "path": "closeout" }),
	);

	assert!(response.success);
	assert!(finalize_response.success);

	DynamicToolHandler::validate_turn_completion(&bridge, "done")
		.expect("closeout completion should allow the turn to complete");

	bridge.apply_closeout().expect("closeout should validate cleanly");

	let comments = tracker.comments.borrow();

	assert_eq!(comments.len(), 1);
	assert!(comments[0].contains("decodex closeout completed"));
	assert!(comments[0].contains("\"record_type\": \"decodex.linear_execution_event\""));
	assert!(comments[0].contains("\"event_type\": \"closeout\""));
	assert!(tracker.state_updates.borrow().is_empty());
}

#[test]
fn closeout_apply_writes_coarse_comment_without_replaying_existing_records() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let mut completed_issue = tests::sample_review_issue();

	completed_issue.state =
		TrackerState { id: String::from("state-done"), name: String::from("Done") };

	completed_issue.labels.push(TrackerLabel {
		id: String::from("label-active"),
		name: tracker::automation_active_label(TEST_SERVICE_ID),
	});

	let tracker = FakeTracker::with_refresh_snapshots(vec![
		vec![completed_issue.clone()],
		vec![completed_issue],
	]);
	let issue = tests::sample_review_issue();
	let workflow = tests::sample_workflow();
	let pr_url = "https://github.com/hack-ink/decodex/pull/260";
	let mut merged_pull_request = tests::sample_pull_request();

	merged_pull_request.url = String::from(pr_url);
	merged_pull_request.state = String::from("MERGED");

	let existing_record = records::append_structured_comment_record(
		"decodex closeout completed",
		&CloseoutRecord {
			record_type: String::from(CLOSEOUT_RECORD_TYPE),
			completed_at: String::from("2026-04-12T00:00:00Z"),
			run_id: String::from("pub-618-attempt-4-123"),
			attempt_number: 4,
			branch_name: String::from("x/decodex-pub-618"),
			pr_url: String::from(pr_url),
		},
	)
	.expect("closeout record should serialize");

	tracker.issue_comments.borrow_mut().insert(
		issue.id.clone(),
		vec![TrackerComment {
			body: existing_record,
			created_at: String::from("2026-04-12T00:00:00Z"),
		}],
	);

	let inspector = FakePullRequestInspector::new(vec![
		Ok(merged_pull_request.clone()),
		Ok(merged_pull_request),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let review_context = tests::sample_closeout_context_in(temp_dir.path(), pr_url);
	let bridge = TrackerToolBridge::with_review_handoff_inspectors(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		Some(TrackerToolBridge::leaked_test_state_store()),
		&inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
		serde_json::json!({
		"pr_url": pr_url,
		"summary": "Merged the approved lane and finished closeout."
		}),
	);

	tests::seed_docs_impact_checkpoint(
		tests::bridge_state_store(&bridge),
		&review_context,
		&issue.id,
		"closeout",
		&tests::sample_local_repo().head_oid,
	);

	let finalize_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		serde_json::json!({ "path": "closeout" }),
	);

	assert!(response.success);
	assert!(finalize_response.success);

	bridge.apply_closeout().expect("closeout should persist a coarse tracker summary");

	assert_eq!(tracker.comments.borrow().len(), 1);
	assert!(
		tracker.label_removals.borrow().is_empty(),
		"closeout writeback should keep ownership until cleanup succeeds"
	);
	assert!(tracker.state_updates.borrow().is_empty());
}
