use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolHandler, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker,
	ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ReviewLevel, TempDir,
	TrackerToolBridge, TurnCompletionStatus, review_policy,
};

#[test]
fn review_completion_skips_clean_checkpoint_when_review_gate_disabled() {
	for completion_path in ["handoff", "repair"] {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let tracker = FakeTracker::new();
		let workflow = tests::sample_workflow();

		if completion_path == "handoff" {
			let issue = tests::sample_issue();
			let inspector = FakePullRequestInspector::new(vec![Ok(tests::sample_pull_request())]);
			let local_repo_inspector =
				FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
			let mut review_context = tests::sample_review_context_in(temp_dir.path());

			review_context.review_level = ReviewLevel::Off;

			let bridge = TrackerToolBridge::with_review_handoff_for_test(
				&tracker,
				&issue,
				&workflow,
				review_context,
				&inspector,
				&local_repo_inspector,
			);
			let response = DynamicToolHandler::handle_call(
				&bridge,
				ISSUE_REVIEW_HANDOFF_TOOL_NAME,
				serde_json::json!({
					"pr_url": "https://github.com/hack-ink/decodex/pull/48",
					"summary": "Ready for review."
				}),
			);

			assert!(response.success, "{completion_path} should not require a clean checkpoint");
		} else {
			let review_issue = tests::sample_review_issue();
			let pr_url = "https://github.com/hack-ink/decodex/pull/242";
			let (repair_inspector, repair_local_repo_inspector) =
				review_policy::sample_review_repair_apply_inspectors(pr_url);
			let mut review_context =
				tests::sample_review_repair_context_in(temp_dir.path(), pr_url);

			review_context.review_level = ReviewLevel::Off;

			let bridge = TrackerToolBridge::with_review_repair_for_test(
				&tracker,
				&review_issue,
				&workflow,
				review_context,
				&repair_inspector,
				&repair_local_repo_inspector,
			);
			let response = DynamicToolHandler::handle_call(
				&bridge,
				ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
				serde_json::json!({
					"pr_url": pr_url,
					"summary": "Addressed the requested review changes."
				}),
			);

			assert!(response.success, "{completion_path} should not require a clean checkpoint");
		}
	}
}

#[test]
fn disabled_review_gate_ignores_stale_review_policy_stop_state() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(Vec::new());
	let mut review_context = tests::sample_review_context_in(temp_dir.path());

	review_context.review_level = ReviewLevel::Off;

	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&inspector,
		&local_repo_inspector,
	);

	tests::write_review_policy_checkpoint(
		&bridge,
		&issue,
		&review_context,
		"handoff",
		"findings",
		&tests::sample_local_repo().head_oid,
		3,
	);

	let completion_status = DynamicToolHandler::classify_turn_completion(&bridge, "done")
		.expect("disabled review gate should ignore stale review stop state");

	assert_eq!(completion_status, TurnCompletionStatus::Continue);
}
