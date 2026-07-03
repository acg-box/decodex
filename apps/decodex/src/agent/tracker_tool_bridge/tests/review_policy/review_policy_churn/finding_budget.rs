use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolContentItem, DynamicToolHandler, FakeLocalRepoInspector,
	FakePullRequestInspector, FakeTracker, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
	ReviewPolicyStopReason, ReviewPolicyStopRequested, TempDir, TrackerToolBridge,
	TurnCompletionStatus, review_policy,
};

#[test]
fn review_checkpoint_findings_continue_until_budget_then_stop() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
		Ok(tests::sample_local_repo()),
	]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context.clone(),
		&pull_request_inspector,
		&local_repo_inspector,
	);

	for expected_round in [1_i64, 2_i64] {
		let response =
			review_policy::submit_findings_review_checkpoint(&bridge, "owned fix still pending");

		assert!(response.success);
		assert_eq!(
			DynamicToolHandler::classify_turn_completion(&bridge, "continue")
				.expect("current_blocker repeats below the convergence budget should continue"),
			TurnCompletionStatus::Continue
		);

		let checkpoint =
			tests::persisted_review_policy_checkpoint(&bridge, &issue, &review_context);

		assert_eq!(checkpoint.phase(), "handoff");
		assert_eq!(checkpoint.status(), "findings");
		assert_eq!(checkpoint.nonclean_rounds(), expected_round);
	}

	let response = review_policy::submit_findings_review_checkpoint(&bridge, "still not converged");

	assert!(!response.success);
	assert!(
		matches!(
			response.content_items.first(),
			Some(DynamicToolContentItem::InputText { text })
				if text.contains("Review churn threshold exceeded")
		),
		"third current_blocker repeat checkpoint should fail immediately: {response:?}"
	);

	let error = DynamicToolHandler::classify_turn_completion(&bridge, "stop")
		.expect_err("third current_blocker repeat checkpoint should stop the lane");
	let stop = error
		.downcast_ref::<ReviewPolicyStopRequested>()
		.expect("stop boundary should expose a typed review policy error");

	assert_eq!(stop.reason, ReviewPolicyStopReason::Exhausted);
	assert_eq!(stop.nonclean_rounds, Some(3));
	assert!(
		stop.fingerprint
			.as_deref()
			.is_some_and(|fingerprint| { fingerprint.starts_with("review_finding:") }),
		"stop should identify the repeated finding fingerprint: {stop:?}"
	);

	let fourth_response = review_policy::submit_findings_review_checkpoint(
		&bridge,
		"attempted fourth findings checkpoint",
	);

	assert!(!fourth_response.success);
	assert!(
		matches!(
			fourth_response.content_items.first(),
			Some(DynamicToolContentItem::InputText { text })
				if text.contains("Review churn threshold already exceeded")
		),
		"fourth consecutive findings checkpoint should be rejected before persistence: {fourth_response:?}"
	);

	let checkpoint = tests::persisted_review_policy_checkpoint(&bridge, &issue, &review_context);

	assert_eq!(checkpoint.status(), "findings");
	assert_eq!(checkpoint.nonclean_rounds(), 3);

	let fenced_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"phase": "implementing",
			"focus": "Continue repairing after review findings.",
			"next_action": "Keep editing the same repair strategy.",
			"blockers": [],
			"evidence": ["The review checkpoint already exceeded the convergence budget."],
			"verification": [],
			"head_sha": tests::sample_local_repo().head_oid,
			"branch": "x/decodex-1"
		}),
	);

	assert!(!fenced_response.success);
	assert!(
		matches!(
			fenced_response.content_items.first(),
			Some(DynamicToolContentItem::InputText { text })
				if text.contains("Review policy stop `review_policy_exhausted` is active")
					&& text.contains("issue_progress_checkpoint")
		),
		"review policy stop should fence mutable progress writes: {fenced_response:?}"
	);
	assert!(
		tracker.comments.borrow().is_empty(),
		"fenced progress checkpoint must not write a tracker comment"
	);
}
