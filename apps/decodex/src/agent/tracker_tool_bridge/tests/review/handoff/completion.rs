use tempfile::TempDir;

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolHandler, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		ISSUE_TERMINAL_FINALIZE_TOOL_NAME, PullRequestDetails, TrackerToolBridge,
		tests::{
			self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker, review_policy,
		},
	},
	state::StateStore,
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
