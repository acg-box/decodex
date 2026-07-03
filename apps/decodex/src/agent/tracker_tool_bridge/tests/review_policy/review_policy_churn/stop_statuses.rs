use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolContentItem, DynamicToolHandler, FakeLocalRepoInspector,
	FakePullRequestInspector, FakeTracker, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
	ReviewPolicyStopReason, ReviewPolicyStopRequested, TempDir, TrackerToolBridge, review_policy,
};

#[test]
fn blocked_review_checkpoint_requires_landing_blocking_route() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context,
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "blocked",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::handoff_review_contract_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["review cannot continue without external evidence"]
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("requires at least one landing-blocking")
	));
}

#[test]
fn review_checkpoint_architecture_and_blocked_statuses_stop_immediately() {
	for (status, expected_reason) in [
		("needs_architecture_review", ReviewPolicyStopReason::ArchitectureReviewRequired),
		("blocked", ReviewPolicyStopReason::Blocked),
	] {
		let tracker = FakeTracker::new();
		let issue = tests::sample_issue();
		let workflow = tests::sample_workflow();
		let temp_dir = TempDir::new().expect("tempdir should create");
		let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
		let local_repo_inspector =
			FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
		let bridge = TrackerToolBridge::with_review_handoff_for_test(
			&tracker,
			&issue,
			&workflow,
			tests::sample_review_context_in(temp_dir.path()),
			&pull_request_inspector,
			&local_repo_inspector,
		);
		let response = DynamicToolHandler::handle_call(
			&bridge,
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			serde_json::json!({
					"reviewer": "independent_fresh_context",
					"status": status,
				"head_sha": tests::sample_local_repo().head_oid,
				"review_contract": review_policy::handoff_review_contract_json(),
				"checks": review_policy::review_checks_json(),
				"evidence": ["requires human follow-up"],
				"finding_routes": review_policy::route_only_review_route_json(if status == "blocked" {
					"landing_blocker"
				} else {
					"architecture_signal"
				})
			}),
		);

		assert!(response.success);

		let error = DynamicToolHandler::classify_turn_completion(&bridge, "stop")
			.expect_err("stop statuses should fail immediately");
		let stop = error
			.downcast_ref::<ReviewPolicyStopRequested>()
			.expect("stop boundary should expose a typed review policy error");

		assert_eq!(stop.reason, expected_reason);
	}
}
