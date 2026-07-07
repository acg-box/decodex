use tempfile::TempDir;

use crate::agent::tracker_tool_bridge::{
	DynamicToolHandler, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME,
	ISSUE_TERMINAL_FINALIZE_TOOL_NAME, PullRequestDetails, RunCompletionDisposition,
	TrackerToolBridge,
	tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker, TEST_SERVICE_ID},
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

	let handoff_fixture =
		tests::persisted_review_lifecycle_handoff_fixture(&bridge, &issue, &review_context);

	assert_eq!(handoff_fixture.pr_url(), "https://github.com/hack-ink/decodex/pull/53");
	assert_eq!(handoff_fixture.pr_head_oid(), "08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
}
