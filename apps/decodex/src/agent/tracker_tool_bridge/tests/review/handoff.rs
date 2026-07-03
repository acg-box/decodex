use std::{fs, path::Path};

use tempfile::TempDir;

use crate::{
	agent::tracker_tool_bridge,
	agent::tracker_tool_bridge::tests::{
		self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker, TEST_SERVICE_ID,
		review_policy,
	},
	agent::tracker_tool_bridge::{
		DynamicToolContentItem, DynamicToolHandler, ISSUE_COMMENT_TOOL_NAME,
		ISSUE_LABEL_ADD_TOOL_NAME, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME, PullRequestDetails, RepositoryIdentity,
		ReviewHandoffWritebackFailed, RunCompletionDisposition, TrackerToolBridge,
	},
	state::{ReviewHandoffMarker, StateStore},
	test_support,
	tracker::{
		TrackerComment,
		records::{self, REVIEW_HANDOFF_RECORD_TYPE, ReviewHandoffRecord},
	},
};

#[test]
fn turn_completion_requires_explicit_terminal_finalize_after_review_handoff() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(vec![Ok(PullRequestDetails {
		head_ref_name: String::from("x/decodex-pub-618"),
		head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		head_repository_name: String::from("decodex"),
		head_repository_owner: String::from("hack-ink"),
		is_draft: false,
		state: String::from("OPEN"),
		base_ref_name: String::from("main"),
		url: String::from("https://github.com/hack-ink/decodex/pull/52"),
	})]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
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
			"pr_url": "https://github.com/hack-ink/decodex/pull/52",
			"summary": "Ready for review."
		}),
	);

	assert!(response.success);

	let error = DynamicToolHandler::validate_turn_completion(&bridge, "done")
		.expect_err("review handoff should still require explicit finalization");

	assert!(error.to_string().contains(ISSUE_TERMINAL_FINALIZE_TOOL_NAME));
	assert!(error.to_string().contains("review_handoff"));
}

#[test]
fn review_handoff_reuses_same_head_clean_checkpoint_artifact_across_attempts() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let first_pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let first_local_repo = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
	let first_context = tests::sample_review_context_in(temp_dir.path());
	let first_bridge = TrackerToolBridge::with_review_handoff_inspectors(
		&tracker,
		&issue,
		&workflow,
		first_context,
		Some(&state_store),
		&first_pull_request_inspector,
		&first_local_repo,
	);
	let checkpoint_response = DynamicToolHandler::handle_call(
		&first_bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "clean",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::handoff_review_contract_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer read the issue contract, current diff, and HEAD"]
		}),
	);

	assert!(checkpoint_response.success);

	let mut second_context = tests::sample_review_context_in(temp_dir.path());

	second_context.run_id = String::from("pub-618-attempt-3-456");
	second_context.attempt_number = 3;

	let pull_request_inspector = FakePullRequestInspector::new(vec![Ok(PullRequestDetails {
		head_ref_name: String::from("x/decodex-pub-618"),
		head_ref_oid: tests::sample_local_repo().head_oid,
		head_repository_name: String::from("decodex"),
		head_repository_owner: String::from("hack-ink"),
		is_draft: false,
		state: String::from("OPEN"),
		base_ref_name: String::from("main"),
		url: String::from("https://github.com/hack-ink/decodex/pull/54"),
	})]);
	let second_local_repo = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
	let second_bridge = TrackerToolBridge::with_review_handoff_inspectors(
		&tracker,
		&issue,
		&workflow,
		second_context.clone(),
		Some(&state_store),
		&pull_request_inspector,
		&second_local_repo,
	);
	let handoff_response = DynamicToolHandler::handle_call(
		&second_bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": "https://github.com/hack-ink/decodex/pull/54",
			"summary": "Ready for review."
		}),
	);

	assert!(handoff_response.success);
	assert!(
		state_store
			.review_policy_checkpoint(
				&second_context.service_id,
				&issue.id,
				&second_context.run_id,
				second_context.attempt_number,
				"handoff",
			)
			.expect("second attempt checkpoint projection should read")
			.is_none()
	);
}

#[test]
fn terminal_finalize_accepts_matching_review_handoff_path() {
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
		url: String::from("https://github.com/hack-ink/decodex/pull/53"),
	};
	let inspector =
		FakePullRequestInspector::new(vec![Ok(pull_request.clone()), Ok(pull_request.clone())]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);
	let run_id = review_context.run_id.clone();
	let attempt_number = review_context.attempt_number;

	tests::write_clean_review_checkpoint(&bridge, &issue, &review_context);

	let review_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": "https://github.com/hack-ink/decodex/pull/53",
			"summary": "Ready for review."
		}),
	);
	let checkpoint_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"phase": "ready_for_review",
			"docs_impact": "none",
			"focus": "Finalize review handoff.",
			"next_action": "Record terminal finalize.",
			"blockers": [],
			"evidence": ["Review handoff recorded."]
		}),
	);
	let finalize_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		serde_json::json!({ "path": "review_handoff" }),
	);

	assert!(review_response.success);
	assert!(checkpoint_response.success);
	assert!(finalize_response.success);
	assert_eq!(
		bridge.finalized_completion_disposition().expect("finalized disposition should resolve"),
		Some(RunCompletionDisposition::ReviewHandoff)
	);

	DynamicToolHandler::validate_turn_completion(&bridge, "done")
		.expect("matching finalization should allow the turn to complete");

	let events = tests::bridge_state_store(&bridge)
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, &run_id, attempt_number)
		.expect("private terminal events should read");

	assert!(events.iter().any(|event| {
		event.event_type() == "review_completion_intent"
			&& event.payload()["path"] == "review_handoff"
			&& event.payload()["pr_url"] == "https://github.com/hack-ink/decodex/pull/53"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "terminal_finalize" && event.payload()["path"] == "review_handoff"
	}));

	let handoff_marker = tests::persisted_review_handoff_marker(&bridge, &issue, &review_context);

	assert_eq!(handoff_marker.pr_url(), "https://github.com/hack-ink/decodex/pull/53");
	assert_eq!(handoff_marker.pr_head_oid(), "08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
}

#[test]
fn terminal_finalize_rejects_review_handoff_when_existing_marker_points_at_different_pr() {
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
		url: String::from("https://github.com/hack-ink/decodex/pull/53"),
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
	tests::bridge_state_store(&bridge)
		.upsert_review_handoff_marker(
			TEST_SERVICE_ID,
			&issue.id,
			&ReviewHandoffMarker::new(
				"old-run",
				1,
				"x/decodex-pub-618",
				"https://github.com/hack-ink/decodex/pull/99",
				"main",
				"x/decodex-pub-618",
				"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
			),
		)
		.expect("existing marker should seed");

	let review_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": "https://github.com/hack-ink/decodex/pull/53",
			"summary": "Ready for review."
		}),
	);
	let checkpoint_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"phase": "ready_for_review",
			"docs_impact": "none",
			"focus": "Finalize review handoff.",
			"next_action": "Record terminal finalize.",
			"blockers": [],
			"evidence": ["Review handoff recorded."]
		}),
	);
	let finalize_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		serde_json::json!({ "path": "review_handoff" }),
	);

	assert!(review_response.success);
	assert!(checkpoint_response.success);
	assert!(!finalize_response.success);
	assert!(matches!(
		finalize_response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("Use explicit review-handoff recovery before rebinding this lane.")
	));
	assert_eq!(
		tests::persisted_review_handoff_marker(&bridge, &issue, &review_context).pr_url(),
		"https://github.com/hack-ink/decodex/pull/99"
	);
}

#[test]
fn terminal_finalize_requires_docs_impact_checkpoint_for_success_paths() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(vec![Ok(PullRequestDetails {
		head_ref_name: String::from("x/decodex-pub-618"),
		head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		head_repository_name: String::from("decodex"),
		head_repository_owner: String::from("hack-ink"),
		is_draft: false,
		state: String::from("OPEN"),
		base_ref_name: String::from("main"),
		url: String::from("https://github.com/hack-ink/decodex/pull/55"),
	})]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
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

	let review_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": "https://github.com/hack-ink/decodex/pull/55",
			"summary": "Ready for review."
		}),
	);
	let finalize_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		serde_json::json!({ "path": "review_handoff" }),
	);

	assert!(review_response.success);
	assert!(!finalize_response.success);
	assert!(matches!(
		finalize_response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("requires a prior `issue_progress_checkpoint` with `docs_impact`")
	));
}

#[test]
fn terminal_finalize_requires_docs_impact_checkpoint_for_current_head() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(vec![Ok(PullRequestDetails {
		head_ref_name: String::from("x/decodex-pub-618"),
		head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		head_repository_name: String::from("decodex"),
		head_repository_owner: String::from("hack-ink"),
		is_draft: false,
		state: String::from("OPEN"),
		base_ref_name: String::from("main"),
		url: String::from("https://github.com/hack-ink/decodex/pull/56"),
	})]);
	let mut updated_local_repo = tests::sample_local_repo();

	updated_local_repo.head_oid = String::from("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");

	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
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

	let checkpoint_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"phase": "ready_for_review",
			"docs_impact": "none",
			"focus": "Finalize review handoff.",
			"next_action": "Record terminal finalize.",
			"blockers": [],
			"evidence": ["Review handoff recorded."]
		}),
	);
	let review_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": "https://github.com/hack-ink/decodex/pull/56",
			"summary": "Ready for review."
		}),
	);
	let finalize_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		serde_json::json!({ "path": "review_handoff" }),
	);

	assert!(checkpoint_response.success);
	assert!(review_response.success);
	assert!(!finalize_response.success);
	assert!(matches!(
		finalize_response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("requires the latest `issue_progress_checkpoint` to record `docs_impact` for the current lane HEAD `deadbeefdeadbeefdeadbeefdeadbeefdeadbeef`")
	));
}

#[test]
fn terminal_finalize_rejects_mismatched_path() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(vec![Ok(PullRequestDetails {
		head_ref_name: String::from("x/decodex-pub-618"),
		head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		head_repository_name: String::from("decodex"),
		head_repository_owner: String::from("hack-ink"),
		is_draft: false,
		state: String::from("OPEN"),
		base_ref_name: String::from("main"),
		url: String::from("https://github.com/hack-ink/decodex/pull/54"),
	})]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
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

	let review_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": "https://github.com/hack-ink/decodex/pull/54",
			"summary": "Ready for review."
		}),
	);
	let finalize_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		serde_json::json!({ "path": "manual_attention" }),
	);

	assert!(review_response.success);
	assert!(!finalize_response.success);
	assert!(matches!(
		finalize_response.content_items.as_slice(),
		[DynamicToolContentItem::InputText{ text }]
			if text.contains(
				"requested path `manual_attention`, but the recorded terminal path is `review_handoff`"
			)
	));
}

#[test]
fn terminal_finalize_accepts_matching_manual_attention_path() {
	let issue = tests::sample_issue();
	let tracker = tests::tracker_with_current_issue_snapshot(&issue);
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
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
	let comment_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_COMMENT_TOOL_NAME,
		tests::manual_attention_comment_args(),
	);
	let checkpoint_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"phase": "blocked",
			"docs_impact": "none",
			"focus": "Manual attention required.",
			"next_action": "Wait for human review.",
			"blockers": ["Human input required."],
			"evidence": ["Manual attention comment recorded."]
		}),
	);
	let finalize_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		serde_json::json!({ "path": "manual_attention" }),
	);

	assert!(label_response.success);
	assert!(comment_response.success);
	assert!(checkpoint_response.success);
	assert!(finalize_response.success);

	DynamicToolHandler::validate_turn_completion(&bridge, "done")
		.expect("matching manual-attention finalization should allow the turn to complete");
}

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

#[test]
fn review_handoff_persists_runtime_state_without_local_marker_cache() {
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
			url: String::from("https://github.com/hack-ink/decodex/pull/150"),
		}),
		Ok(PullRequestDetails {
			head_ref_name: String::from("x/decodex-pub-618"),
			head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_repository_name: String::from("decodex"),
			head_repository_owner: String::from("hack-ink"),
			is_draft: false,
			state: String::from("OPEN"),
			base_ref_name: String::from("main"),
			url: String::from("https://github.com/hack-ink/decodex/pull/150"),
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
			"pr_url": "https://github.com/hack-ink/decodex/pull/150",
			"summary": "Ready for review."
		}),
	);

	assert!(response.success);

	bridge
		.apply_review_handoff()
		.expect("runtime state persistence should not depend on local marker files");

	assert_eq!(tracker.state_updates.borrow().as_slice(), ["state-review"]);
	assert_eq!(tracker.comments.borrow().len(), 1);
	assert_eq!(
		tests::persisted_review_handoff_marker(
			&bridge,
			&issue,
			&tests::sample_review_context_in(temp_dir.path())
		)
		.pr_url(),
		"https://github.com/hack-ink/decodex/pull/150"
	);
}

fn review_handoff_pr_details(
	url: &str,
	head_ref_name: &str,
	head_ref_oid: &str,
	owner: &str,
	repository: &str,
	base_ref_name: &str,
	is_draft: bool,
) -> PullRequestDetails {
	PullRequestDetails {
		head_ref_name: String::from(head_ref_name),
		head_ref_oid: String::from(head_ref_oid),
		head_repository_name: String::from(repository),
		head_repository_owner: String::from(owner),
		is_draft,
		state: String::from("OPEN"),
		base_ref_name: String::from(base_ref_name),
		url: String::from(url),
	}
}

#[test]
fn rejects_invalid_pull_requests_for_review_handoff() {
	for (case_name, pull_request, expected_error) in [
		(
			"another branch",
			review_handoff_pr_details(
				"https://github.com/hack-ink/decodex/pull/43",
				"x/decodex-pub-999",
				"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
				"hack-ink",
				"decodex",
				"main",
				false,
			),
			None,
		),
		(
			"draft pull request",
			review_handoff_pr_details(
				"https://github.com/hack-ink/decodex/pull/44",
				"x/decodex-pub-618",
				"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
				"hack-ink",
				"decodex",
				"main",
				true,
			),
			None,
		),
		(
			"stale PR head",
			review_handoff_pr_details(
				"https://github.com/hack-ink/decodex/pull/45",
				"x/decodex-pub-618",
				"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
				"hack-ink",
				"decodex",
				"main",
				false,
			),
			None,
		),
		(
			"another repository",
			review_handoff_pr_details(
				"https://github.com/someone-else/decodex-fork/pull/46",
				"x/decodex-pub-618",
				"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
				"someone-else",
				"decodex-fork",
				"main",
				false,
			),
			None,
		),
		(
			"non-default target branch",
			review_handoff_pr_details(
				"https://github.com/hack-ink/decodex/pull/47",
				"x/decodex-pub-618",
				"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
				"hack-ink",
				"decodex",
				"release/1.x",
				false,
			),
			Some("retained review lanes must target the repository default branch `main`"),
		),
	] {
		let tracker = FakeTracker::new();
		let issue = tests::sample_issue();
		let workflow = tests::sample_workflow();
		let pr_url = pull_request.url.clone();
		let inspector = FakePullRequestInspector::new(vec![Ok(pull_request)]);
		let local_repo_inspector =
			FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
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
			ISSUE_REVIEW_HANDOFF_TOOL_NAME,
			serde_json::json!({
				"pr_url": pr_url,
				"summary": "Ready for review."
			}),
		);

		assert!(!response.success, "{case_name}");
		assert!(tracker.comments.borrow().is_empty(), "{case_name}");
		assert!(tracker.state_updates.borrow().is_empty(), "{case_name}");

		if let Some(expected_error) = expected_error {
			assert!(
				matches!(
					response.content_items.as_slice(),
					[DynamicToolContentItem::InputText{ text }] if text.contains(expected_error)
				),
				"{case_name}"
			);
		}

		assert!(bridge.apply_review_handoff().is_err(), "{case_name}");
	}
}

#[test]
fn parses_credentialed_https_github_remote() {
	let repository = tracker_tool_bridge::parse_github_repository_identity(
		"https://x-access-token@github.com/hack-ink/decodex.git",
	)
	.expect("credentialed GitHub remote should parse");

	assert_eq!(
		repository,
		RepositoryIdentity { owner: String::from("hack-ink"), name: String::from("decodex") }
	);
}

#[test]
fn parses_default_branch_from_ls_remote_symref_output() {
	let parsed = tracker_tool_bridge::parse_remote_head_symref_output(
		"ref: refs/heads/main\tHEAD\n9c0ffee\tHEAD\n9c0ffee\trefs/heads/main\n",
	);

	assert_eq!(parsed.as_deref(), Some("main"));
}

#[test]
fn ignores_non_head_lines_when_parsing_default_branch_from_ls_remote_output() {
	let parsed = tracker_tool_bridge::parse_remote_head_symref_output(
		"9c0ffee\trefs/heads/main\n9c0ffee\trefs/heads/release/1.x\n",
	);

	assert_eq!(parsed, None);
}

#[test]
fn resolve_lane_default_branch_prefers_cached_origin_head() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let remote_root = temp_dir.path().join("origin.git");
	let repo_root = temp_dir.path().join("repo");

	run_git_for_handoff(
		temp_dir.path(),
		&[
			"init",
			"--bare",
			"--initial-branch",
			"main",
			remote_root.to_str().expect("remote path utf-8"),
		],
	);

	fs::create_dir_all(&repo_root).expect("repo root should exist");

	run_git_for_handoff(&repo_root, &["init", "--initial-branch", "main"]);
	run_git_for_handoff(&repo_root, &["config", "user.name", "Decodex Tests"]);
	run_git_for_handoff(&repo_root, &["config", "user.email", "decodex-tests@example.com"]);
	run_git_for_handoff(
		&repo_root,
		&["remote", "add", "origin", remote_root.to_str().expect("remote path utf-8")],
	);

	fs::write(repo_root.join("README.md"), "seed\n").expect("seed file should write");

	run_git_for_handoff(&repo_root, &["add", "README.md"]);
	run_git_for_handoff(&repo_root, &["commit", "-m", "seed"]);
	run_git_for_handoff(&repo_root, &["push", "-u", "origin", "main"]);
	run_git_for_handoff(&repo_root, &["checkout", "-b", "trunk"]);
	run_git_for_handoff(&repo_root, &["push", "origin", "trunk"]);
	run_git_for_handoff(&remote_root, &["symbolic-ref", "HEAD", "refs/heads/trunk"]);
	run_git_for_handoff(
		&repo_root,
		&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"],
	);
	run_git_for_handoff(&repo_root, &["checkout", "main"]);

	let resolved = tracker_tool_bridge::resolve_lane_default_branch(&repo_root)
		.expect("default branch should resolve");

	assert_eq!(resolved, "main");
}

#[test]
fn resolve_lane_default_branch_uses_remote_head_when_local_cache_is_missing() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let remote_root = temp_dir.path().join("origin.git");
	let repo_root = temp_dir.path().join("repo");

	run_git_for_handoff(
		temp_dir.path(),
		&[
			"init",
			"--bare",
			"--initial-branch",
			"main",
			remote_root.to_str().expect("remote path utf-8"),
		],
	);

	fs::create_dir_all(&repo_root).expect("repo root should exist");

	run_git_for_handoff(&repo_root, &["init", "--initial-branch", "main"]);
	run_git_for_handoff(&repo_root, &["config", "user.name", "Decodex Tests"]);
	run_git_for_handoff(&repo_root, &["config", "user.email", "decodex-tests@example.com"]);
	run_git_for_handoff(
		&repo_root,
		&["remote", "add", "origin", remote_root.to_str().expect("remote path utf-8")],
	);

	fs::write(repo_root.join("README.md"), "seed\n").expect("seed file should write");

	run_git_for_handoff(&repo_root, &["add", "README.md"]);
	run_git_for_handoff(&repo_root, &["commit", "-m", "seed"]);
	run_git_for_handoff(&repo_root, &["push", "-u", "origin", "main"]);
	run_git_for_handoff(&repo_root, &["checkout", "-b", "trunk"]);
	run_git_for_handoff(&repo_root, &["push", "origin", "trunk"]);
	run_git_for_handoff(&remote_root, &["symbolic-ref", "HEAD", "refs/heads/trunk"]);
	run_git_for_handoff(&repo_root, &["checkout", "main"]);

	let resolved = tracker_tool_bridge::resolve_lane_default_branch(&repo_root)
		.expect("default branch should resolve");

	assert_eq!(resolved, "trunk");
}

#[test]
fn resolve_lane_default_branch_uses_cached_origin_head_without_reachable_remote() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let remote_root = temp_dir.path().join("origin.git");
	let repo_root = temp_dir.path().join("repo");

	run_git_for_handoff(
		temp_dir.path(),
		&[
			"init",
			"--bare",
			"--initial-branch",
			"main",
			remote_root.to_str().expect("remote path utf-8"),
		],
	);

	fs::create_dir_all(&repo_root).expect("repo root should exist");

	run_git_for_handoff(&repo_root, &["init", "--initial-branch", "main"]);
	run_git_for_handoff(&repo_root, &["config", "user.name", "Decodex Tests"]);
	run_git_for_handoff(&repo_root, &["config", "user.email", "decodex-tests@example.com"]);
	run_git_for_handoff(
		&repo_root,
		&["remote", "add", "origin", remote_root.to_str().expect("remote path utf-8")],
	);

	fs::write(repo_root.join("README.md"), "seed\n").expect("seed file should write");

	run_git_for_handoff(&repo_root, &["add", "README.md"]);
	run_git_for_handoff(&repo_root, &["commit", "-m", "seed"]);
	run_git_for_handoff(&repo_root, &["push", "-u", "origin", "main"]);
	run_git_for_handoff(
		&repo_root,
		&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"],
	);
	run_git_for_handoff(
		&repo_root,
		&[
			"remote",
			"set-url",
			"origin",
			temp_dir.path().join("missing-origin.git").to_str().expect("missing remote path utf-8"),
		],
	);

	let resolved = tracker_tool_bridge::resolve_lane_default_branch(&repo_root)
		.expect("default branch should resolve");

	assert_eq!(resolved, "main");
}

fn run_git_for_handoff(cwd: &Path, args: &[&str]) {
	let status = test_support::hermetic_git_command()
		.arg("-C")
		.arg(cwd)
		.args(args)
		.status()
		.expect("git should run");

	assert!(status.success(), "git {:?} should succeed in `{}`", args, cwd.display());
}

#[test]
fn publishes_protocol_safe_tool_names() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
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
	let tool_specs = DynamicToolHandler::tool_specs(&bridge);

	assert!(!tool_specs.is_empty());
	assert!(tool_specs.into_iter().all(|tool| {
		tool.name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
	}));
}
