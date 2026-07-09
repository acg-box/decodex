use tempfile::TempDir;

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolContentItem, DynamicToolHandler, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME, PullRequestDetails,
		TrackerToolBridge,
		tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker},
	},
	orchestrator::VALIDATION_EVIDENCE_EVENT_TYPE,
};

#[test]
fn terminal_finalize_requires_openwiki_impact_checkpoint_for_success_paths() {
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
			if text.contains("requires a prior `issue_progress_checkpoint` with `openwiki_impact`")
	));
}

#[test]
fn terminal_finalize_still_requires_openwiki_impact_after_validation_evidence_pass() {
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
		url: String::from("https://github.com/hack-ink/decodex/pull/57"),
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
	tests::bridge_state_store(&bridge)
		.append_private_execution_event(
			&review_context.service_id,
			&issue.id,
			&review_context.run_id,
			review_context.attempt_number,
			VALIDATION_EVIDENCE_EVENT_TYPE,
			serde_json::json!({
				"schema": "decodex.validation_evidence/1",
				"decision": "pass",
				"reason_code": "accepted",
				"objective_coverage": { "covered": true, "checkpoint_record_id": null },
				"effective_delta": { "present": true, "changed_surfaces": ["ready.txt"] }
			}),
		)
		.expect("validation evidence should seed");

	let review_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": "https://github.com/hack-ink/decodex/pull/57",
			"summary": "Ready for review after validation evidence."
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
			if text.contains("requires a prior `issue_progress_checkpoint` with `openwiki_impact`")
	));
}

#[test]
fn terminal_finalize_requires_openwiki_impact_checkpoint_for_current_head() {
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
		url: String::from("https://github.com/hack-ink/decodex/pull/56"),
	};
	let inspector = FakePullRequestInspector::new(vec![Ok(pull_request.clone()), Ok(pull_request)]);
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

	let checkpoint_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"phase": "ready_for_review",
			"openwiki_impact": "none",
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
			if text.contains("requires the latest `issue_progress_checkpoint` to record `openwiki_impact` for the current lane HEAD `deadbeefdeadbeefdeadbeefdeadbeefdeadbeef`")
	));
}
