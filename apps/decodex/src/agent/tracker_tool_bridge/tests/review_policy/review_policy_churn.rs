use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolContentItem, DynamicToolHandler, FakeLocalRepoInspector,
	FakePullRequestInspector, FakeTracker, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
	ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, ReviewExecutionMode, ReviewHandoffContext,
	ReviewPolicyStopReason, ReviewPolicyStopRequested, TempDir, TrackerToolBridge,
	TurnCompletionStatus, Value, review_policy,
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

#[test]
fn review_checkpoint_distinct_findings_do_not_inherit_old_churn() {
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

	for _round in 1..=2 {
		let response =
			review_policy::submit_findings_review_checkpoint(&bridge, "same finding still pending");

		assert!(response.success);
	}

	let distinct_findings = review_policy::accepted_review_findings_with_summary_json(
		"Distinct reviewer finding",
		"Repair the separate accepted issue before requesting another checkpoint.",
		12,
	);
	let response = review_policy::submit_findings_review_checkpoint_with_findings(
		&bridge,
		"new finding discovered after the earlier one was repaired",
		distinct_findings,
	);

	assert!(response.success, "new fingerprints should not trip old churn: {response:?}");

	let checkpoint = tests::persisted_review_policy_checkpoint(&bridge, &issue, &review_context);
	let details =
		serde_json::from_str::<Value>(checkpoint.details_json()).expect("details should be json");
	let finding_policy = &details["finding_policy"];
	let records = finding_policy["findings"].as_array().expect("finding records should persist");

	assert_eq!(checkpoint.nonclean_rounds(), 1);
	assert_eq!(
		finding_policy["active_fingerprints"].as_array().expect("active fingerprints").len(),
		1
	);
	assert!(records.iter().any(|record| {
		record["title"] == "Accepted reviewer finding"
			&& record["status"] == "resolved"
			&& record["repeat_count"] == 2
	}));
	assert!(records.iter().any(|record| {
		record["title"] == "Distinct reviewer finding"
			&& record["status"] == "open"
			&& record["repeat_count"] == 1
	}));
}

#[test]
fn review_checkpoint_clean_resets_nonclean_rounds_before_next_findings() {
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

	for status in ["findings", "findings", "clean", "findings"] {
		let response = DynamicToolHandler::handle_call(
			&bridge,
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			serde_json::json!({
				"reviewer": "independent_fresh_context",
				"status": status,
				"head_sha": tests::sample_local_repo().head_oid,
				"review_contract": review_policy::handoff_review_contract_json(),
				"checks": review_policy::review_checks_json(),
				"evidence": ["review evidence"],
				"accepted_findings": review_policy::accepted_review_findings_for_status_json(status)
			}),
		);

		assert!(response.success);
	}

	let checkpoint = tests::persisted_review_policy_checkpoint(&bridge, &issue, &review_context);

	assert_eq!(checkpoint.status(), "findings");
	assert_eq!(checkpoint.nonclean_rounds(), 1);
	assert_eq!(
		DynamicToolHandler::classify_turn_completion(&bridge, "continue")
			.expect("findings after a clean checkpoint should continue"),
		TurnCompletionStatus::Continue
	);
}

#[test]
fn review_checkpoint_does_not_depend_on_tracker_comment_write() {
	let tracker = FakeTracker::with_comment_error("tracker write failed");
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
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
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "findings",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::handoff_review_contract_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["tracker write failed before checkpoint persisted"],
			"accepted_findings": review_policy::accepted_review_findings_json()
		}),
	);

	assert!(response.success);
	assert!(tracker.comments.borrow().is_empty());

	let checkpoint = tests::persisted_review_policy_checkpoint(&bridge, &issue, &review_context);

	assert_eq!(checkpoint.nonclean_rounds(), 1);
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

#[test]
fn review_checkpoint_phase_switch_resets_nonclean_rounds() {
	let tracker = FakeTracker::new();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let repair_context = tests::sample_review_repair_context_in(
		temp_dir.path(),
		"https://github.com/hack-ink/decodex/pull/242",
	);
	let issue = tests::sample_review_issue();
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo())]);
	let bridge = TrackerToolBridge::with_review_repair_for_test(
		&tracker,
		&issue,
		&workflow,
		repair_context.clone(),
		&pull_request_inspector,
		&local_repo_inspector,
	);

	tests::write_review_policy_checkpoint(
		&bridge,
		&issue,
		&ReviewHandoffContext { mode: ReviewExecutionMode::Handoff, ..repair_context.clone() },
		"handoff",
		"findings",
		&tests::sample_local_repo().head_oid,
		2,
	);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "findings",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::repair_review_contract_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh repair-phase review found accepted work"],
			"accepted_findings": review_policy::accepted_review_findings_json()
		}),
	);

	assert!(response.success);

	let checkpoint = tests::persisted_review_policy_checkpoint(&bridge, &issue, &repair_context);

	assert_eq!(checkpoint.phase(), "repair");
	assert_eq!(checkpoint.nonclean_rounds(), 1);
}
