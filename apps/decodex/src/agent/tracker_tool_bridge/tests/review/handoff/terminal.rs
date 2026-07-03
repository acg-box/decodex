use tempfile::TempDir;

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolContentItem, DynamicToolHandler, ISSUE_COMMENT_TOOL_NAME,
		ISSUE_LABEL_ADD_TOOL_NAME, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME, PullRequestDetails,
		RunCompletionDisposition, TrackerToolBridge,
		tests::{
			self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker, TEST_SERVICE_ID,
		},
	},
	state::ReviewHandoffMarker,
};

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
