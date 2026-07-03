use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolHandler, FakeLocalRepoInspector, FakePullRequestInspector, FakeTracker,
	ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, TempDir, TrackerToolBridge, TurnCompletionStatus, Value,
	review_policy,
};

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
