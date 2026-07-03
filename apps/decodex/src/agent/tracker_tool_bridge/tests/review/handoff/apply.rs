use tempfile::TempDir;

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolHandler, ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		PullRequestDetails, ReviewHandoffWritebackFailed, TrackerToolBridge,
		tests::{
			self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker, TEST_SERVICE_ID,
		},
	},
	tracker::{
		TrackerComment,
		records::{self, REVIEW_HANDOFF_RECORD_TYPE, ReviewHandoffRecord},
	},
};

#[test]
fn rejects_review_handoff_apply_when_lane_head_changes_after_recording() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
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
			url: String::from("https://github.com/hack-ink/decodex/pull/47"),
		}),
		Ok(PullRequestDetails {
			head_ref_name: String::from("x/decodex-pub-618"),
			head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_repository_name: String::from("decodex"),
			head_repository_owner: String::from("hack-ink"),
			is_draft: false,
			state: String::from("OPEN"),
			base_ref_name: String::from("main"),
			url: String::from("https://github.com/hack-ink/decodex/pull/47"),
		}),
	]);
	let mut updated_local_repo = tests::sample_local_repo();

	updated_local_repo.head_oid = String::from("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");

	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
		Ok(updated_local_repo),
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
			"pr_url": "https://github.com/hack-ink/decodex/pull/47",
			"summary": "Ready for review."
		}),
	);

	assert!(response.success);

	let error = bridge
		.apply_review_handoff()
		.expect_err("writeback should revalidate the current lane head");

	assert!(error.to_string().contains("Push the latest lane commit before review handoff."));
	assert!(tracker.comments.borrow().is_empty());
	assert!(tracker.state_updates.borrow().is_empty());
}

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

#[test]
fn keeps_review_handoff_marker_when_state_transition_fails_after_tracker_record_write() {
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
			.review_handoff_marker(TEST_SERVICE_ID, &issue.id, "x/decodex-pub-618")
			.expect("runtime handoff marker read should succeed")
			.expect("partial review handoff should keep the retained marker")
			.pr_url(),
		"https://github.com/hack-ink/decodex/pull/49"
	);
}

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
			.review_handoff_marker(TEST_SERVICE_ID, &issue.id, "x/decodex-pub-618")
			.expect("runtime handoff marker read should succeed")
			.expect("tracker writeback failure should keep durable handoff marker")
			.pr_url(),
		"https://github.com/hack-ink/decodex/pull/50"
	);
}

#[test]
fn review_handoff_writeback_replaces_private_summary_with_public_fallback() {
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
		url: String::from("https://github.com/hack-ink/decodex/pull/151"),
	};
	let inspector =
		FakePullRequestInspector::new(vec![Ok(pull_request.clone()), Ok(pull_request.clone())]);
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
			"pr_url": pull_request.url,
			"summary": "Completed from /Users/example/repo with GITHUB_PAT_Y configured."
		}),
	);

	assert!(response.success);

	bridge.apply_review_handoff().expect("private-looking summaries should use fallback text");

	let comments = tracker.comments.borrow();
	let comment = comments.first().expect("review handoff comment should write");

	assert!(comment.contains("Implementation completed and the PR is ready for review."));
	assert!(!comment.contains("/Users/example/repo"));
	assert!(!comment.contains("GITHUB_PAT_Y"));

	let record = records::parse_linear_execution_event_record(comment)
		.expect("review handoff should write a valid Linear execution event");

	assert_eq!(
		record.summary.as_deref(),
		Some("Implementation completed and the PR is ready for review.")
	);
	assert_eq!(record.pr_url.as_deref(), Some("https://github.com/hack-ink/decodex/pull/151"));
}

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
			.review_handoff_marker(TEST_SERVICE_ID, &issue.id, "x/decodex-pub-618")
			.expect("runtime handoff marker read should succeed")
			.is_none()
	);
}
