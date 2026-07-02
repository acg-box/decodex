use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolContentItem, DynamicToolHandler, FakeLocalRepoInspector,
	FakePullRequestInspector, FakeTracker, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
	ISSUE_REVIEW_HANDOFF_TOOL_NAME, ReviewCheckpointArtifactLookup, TempDir, TrackerToolBridge,
	TurnCompletionStatus, Value, review_policy,
};

#[test]
fn review_checkpoint_rejects_review_blocking_local_changes() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let local_repo_inspector =
		FakeLocalRepoInspector::new(vec![Ok(review_policy::sample_dirty_local_repo())]);
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
			"status": "clean",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::handoff_review_contract_json(),
			"review_cost_control": review_policy::compact_review_cost_control_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["review tried to bind a dirty worktree"]
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("requires a clean committed lane HEAD")
				&& text.contains("M apps/decodex/src/agent/tracker_tool_bridge/tools.rs")
				&& text.contains("?? apps/decodex/src/agent/new_review_surface.rs")
	));
	assert!(
		tests::bridge_state_store(&bridge)
			.review_checkpoint_artifact(ReviewCheckpointArtifactLookup {
				project_id: &review_context.service_id,
				issue_id: &issue.id,
				phase: "handoff",
				review_level: review_context.review_level.as_str(),
				head_sha: &tests::sample_local_repo().head_oid,
			})
			.expect("artifact lookup should succeed")
			.is_none(),
		"dirty checkpoint attempts must not persist reusable review evidence"
	);
}

#[test]
fn independent_review_checkpoint_findings_store_accepted_repair_guidance() {
	let tracker = FakeTracker::new();
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
			"evidence": ["fresh reviewer found one accepted repair item"],
			"accepted_findings": review_policy::accepted_review_findings_json()
		}),
	);

	assert!(response.success);
	assert_eq!(
		DynamicToolHandler::classify_turn_completion(&bridge, "continue")
			.expect("first accepted findings round should continue"),
		TurnCompletionStatus::Continue
	);

	let checkpoint = tests::persisted_review_policy_checkpoint(&bridge, &issue, &review_context);
	let details =
		serde_json::from_str::<Value>(checkpoint.details_json()).expect("details should be json");

	assert_eq!(checkpoint.status(), "findings");
	assert_eq!(checkpoint.nonclean_rounds(), 1);
	assert_eq!(details["accepted_findings"][0]["severity"], "medium");
	assert_eq!(details["accepted_findings"][0]["kind"], "accepted_finding");
	assert_eq!(details["accepted_findings"][0]["line_range"]["start"], 1);
	assert!(
		details["accepted_findings"][0]["fingerprint"]
			.as_str()
			.is_some_and(|fingerprint| fingerprint.starts_with("review_finding:"))
	);
	assert_eq!(
		details["accepted_findings"][0]["guidance"],
		"Repair the accepted issue before requesting another review checkpoint."
	);
	assert_eq!(details["finding_routes"][0]["route"], "current_blocker");
	assert_eq!(
		details["finding_routes"][0]["finding_fingerprint"],
		details["accepted_findings"][0]["fingerprint"]
	);
	assert_eq!(details["finding_route_summary"]["route_counts"][0]["route"], "current_blocker");
}

#[test]
fn review_checkpoint_rejected_finding_is_non_actionable_and_can_handoff_cleanly() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let inspector = FakePullRequestInspector::new(vec![Ok(tests::sample_pull_request())]);
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
	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "clean",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::handoff_review_contract_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["only rejected non-actionable feedback remained"],
			"rejected_findings": [{
				"severity": "low",
				"summary": "The reviewer requested a migration test.",
				"rejection_reason": "No migration path changed in the current diff.",
				"evidence": ["The runtime store column is additive and defaults existing rows."],
				"file": "apps/decodex/src/state/internal.rs",
				"line": 1
			}]
		}),
	);

	assert!(response.success);

	let handoff_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		serde_json::json!({
			"pr_url": "https://github.com/hack-ink/decodex/pull/48",
			"summary": "Rejected non-actionable review feedback and prepared handoff."
		}),
	);

	assert!(handoff_response.success);

	tests::assert_review_policy_checkpoint_cleared(&bridge, &issue, &review_context);
}

#[test]
fn clean_review_checkpoint_records_non_current_routes_without_churn() {
	let tracker = FakeTracker::new();
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

	for _round in 0..2 {
		let response = DynamicToolHandler::handle_call(
			&bridge,
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			serde_json::json!({
				"reviewer": "independent_fresh_context",
				"status": "clean",
				"head_sha": tests::sample_local_repo().head_oid,
				"review_contract": review_policy::handoff_review_contract_json(),
				"checks": review_policy::review_checks_json(),
				"evidence": ["fresh reviewer found only non-current follow-up work"],
				"finding_routes": review_policy::route_only_review_route_json("follow_up")
			}),
		);

		assert!(response.success);
	}

	let checkpoint = tests::persisted_review_policy_checkpoint(&bridge, &issue, &review_context);
	let details =
		serde_json::from_str::<Value>(checkpoint.details_json()).expect("details should be json");

	assert_eq!(checkpoint.status(), "clean");
	assert_eq!(checkpoint.nonclean_rounds(), 0);
	assert_eq!(
		details["finding_policy"]["active_fingerprints"]
			.as_array()
			.expect("active fingerprints should be an array")
			.len(),
		0
	);
	assert_eq!(details["finding_route_summary"]["route_counts"][0]["route"], "follow_up");
}

#[test]
fn review_checkpoint_rejects_high_risk_invalid_route() {
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
			"status": "clean",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::handoff_review_contract_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer disputed a severe live-mutation risk"],
			"finding_routes": [{
				"route": "invalid_or_unsubstantiated",
				"severity": "high",
				"risk_tier": "high",
				"summary": "Reviewer alleged a high-risk live mutation.",
				"evidence": ["The reviewer did not provide enough source evidence."],
				"resolver": "agent",
				"next_action": "Route to needs_evidence with source proof instead of invalidating it."
			}]
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("cannot route high-severity or high-risk")
	));
}

#[test]
fn review_checkpoint_rejects_current_blocker_without_accepted_binding() {
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
			"status": "findings",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::handoff_review_contract_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer tried to create an unbound current blocker route"],
			"finding_routes": [{
				"route": "current_blocker",
				"severity": "medium",
				"risk_tier": "medium",
				"summary": "Unbound current blocker route.",
				"evidence": ["The route has no accepted finding binding."],
				"resolver": "agent",
				"next_action": "Bind current blockers to accepted findings before repair."
			}]
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("`finding_routes.route` `current_blocker` must bind to an `accepted_findings` item")
	));
}

#[test]
fn review_checkpoint_rejects_out_of_range_accepted_route_binding() {
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
			"status": "findings",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::handoff_review_contract_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer tried to bind a route to a missing accepted finding"],
			"accepted_findings": review_policy::accepted_review_findings_json(),
			"finding_routes": [{
				"route": "current_blocker",
				"severity": "medium",
				"risk_tier": "medium",
				"summary": "Out-of-range accepted finding binding.",
				"evidence": ["Only one accepted finding exists."],
				"resolver": "agent",
				"next_action": "Bind to an existing accepted finding index.",
				"finding_source": "accepted_findings",
				"finding_index": 99
			}]
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("`finding_routes.finding_index` `99` does not match any accepted finding")
	));
}

#[test]
fn review_checkpoint_rejects_out_of_range_rejected_route_binding() {
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
			"status": "clean",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::handoff_review_contract_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer tried to bind a route to a missing rejected finding"],
			"rejected_findings": [{
				"severity": "low",
				"summary": "Reviewer requested unrelated follow-up work.",
				"rejection_reason": "The request is outside the current issue contract.",
				"evidence": ["The current diff does not touch that surface."]
			}],
			"finding_routes": [{
				"route": "reviewer_rubric_gap",
				"severity": "low",
				"risk_tier": "low",
				"summary": "Out-of-range rejected finding binding.",
				"evidence": ["Only one rejected finding exists."],
				"resolver": "reviewer",
				"next_action": "Bind to an existing rejected finding index.",
				"finding_source": "rejected_findings",
				"finding_index": 99
			}]
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("`finding_routes.finding_index` `99` does not match any rejected finding")
	));
}

#[test]
fn review_checkpoint_rejects_bound_high_severity_invalid_route() {
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
			"status": "findings",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::handoff_review_contract_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer disputed a severe accepted finding"],
			"accepted_findings": [{
				"severity": "high",
				"summary": "Accepted reviewer finding reports a high severity regression.",
				"evidence": ["The reviewer evidence points at the current lane head."],
				"file": "apps/decodex/src/agent/tracker_tool_bridge/tools.rs",
				"line": 1,
				"guidance": "Repair the accepted high severity regression."
			}],
			"finding_routes": [{
				"route": "invalid_or_unsubstantiated",
				"severity": "low",
				"risk_tier": "low",
				"summary": "Route tries to downgrade the accepted finding.",
				"evidence": ["The bound accepted finding is high severity."],
				"resolver": "agent",
				"next_action": "Route to needs_evidence or a landing blocker instead.",
				"finding_source": "accepted_findings",
				"finding_index": 0
			}]
		}),
	);

	assert!(!response.success);
	assert!(matches!(
		response.content_items.as_slice(),
		[DynamicToolContentItem::InputText { text }]
			if text.contains("cannot route high-severity or high-risk")
	));
}
