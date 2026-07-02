use crate::agent::tracker_tool_bridge::tests::{
	self, DynamicToolContentItem, DynamicToolHandler, FakeLocalRepoInspector,
	FakePullRequestInspector, FakeTracker, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, TempDir,
	TrackerToolBridge, review_policy,
};

#[test]
fn compact_review_checkpoint_fails_closed_without_current_validation_or_with_high_risk_surface() {
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
	let mut cost_control = review_policy::compact_review_cost_control_json();

	cost_control["current_head_evidence"] = serde_json::json!(false);
	cost_control["validation_backed"] = serde_json::json!(false);
	cost_control["validation_current"] = serde_json::json!(false);
	cost_control["evidence_sufficient"] = serde_json::json!(false);
	cost_control["high_risk_surfaces"] = serde_json::json!([
		"docs policy surface without matching validation evidence",
		"configuration surface without matching validation evidence",
		"public API surface without matching validation evidence",
		"security surface without matching validation evidence",
		"data/privacy surface without matching validation evidence"
	]);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "clean",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::low_risk_handoff_review_contract_json(),
			"review_cost_control": cost_control,
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer tried to claim compact review without sufficient validation"]
		}),
	);

	assert!(!response.success);

	match &response.content_items[..] {
		[DynamicToolContentItem::InputText { text }] => {
			assert!(text.contains("full review is required"));
			assert!(text.contains("high_risk_surfaces_present"));
			assert!(text.contains("missing_current_head_evidence"));
			assert!(text.contains("missing_validation_evidence"));
			assert!(text.contains("stale_validation_evidence"));
			assert!(text.contains("weak_evidence"));
		},
		other => panic!("unexpected response content: {other:?}"),
	}
}

#[test]
fn compact_review_checkpoint_fails_closed_with_stale_validation_evidence() {
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
	let mut cost_control = review_policy::compact_review_cost_control_json();

	cost_control["validation_current"] = serde_json::json!(false);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "clean",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::low_risk_handoff_review_contract_json(),
			"review_cost_control": cost_control,
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer tried to claim compact review with validation from an older head"]
		}),
	);

	assert!(!response.success);

	match &response.content_items[..] {
		[DynamicToolContentItem::InputText { text }] => {
			assert!(text.contains("full review is required"));
			assert!(text.contains("stale_validation_evidence"));
		},
		other => panic!("unexpected response content: {other:?}"),
	}
}

#[test]
fn compact_review_checkpoint_fails_closed_with_weak_evidence() {
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
	let mut cost_control = review_policy::compact_review_cost_control_json();

	cost_control["evidence_sufficient"] = serde_json::json!(false);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "clean",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::low_risk_handoff_review_contract_json(),
			"review_cost_control": cost_control,
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer tried to claim compact review with weak current-head evidence"]
		}),
	);

	assert!(!response.success);

	match &response.content_items[..] {
		[DynamicToolContentItem::InputText { text }] => {
			assert!(text.contains("full review is required"));
			assert!(text.contains("weak_evidence"));
		},
		other => panic!("unexpected response content: {other:?}"),
	}
}

#[test]
fn compact_review_checkpoint_fails_closed_for_non_low_risk_classification() {
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
	let mut cost_control = review_policy::compact_review_cost_control_json();

	cost_control["risk_class"] = serde_json::json!("localized");

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "clean",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::handoff_review_contract_json(),
			"review_cost_control": cost_control,
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer tried to claim compact review for a localized-risk lane"]
		}),
	);

	assert!(!response.success);

	match &response.content_items[..] {
		[DynamicToolContentItem::InputText { text }] => {
			assert!(text.contains("full review is required"));
			assert!(text.contains("review_contract_risk_tier_not_low"));
			assert!(text.contains("review_cost_risk_class_not_low"));
		},
		other => panic!("unexpected response content: {other:?}"),
	}
}

#[test]
fn compact_review_checkpoint_fails_closed_for_accepted_findings_and_blocking_routes() {
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
			"review_contract": review_policy::low_risk_handoff_review_contract_json(),
			"review_cost_control": review_policy::compact_review_cost_control_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer tried to keep compact review after accepting a current blocker"],
			"accepted_findings": review_policy::accepted_review_findings_json(),
			"finding_routes": [{
				"route": "current_blocker",
				"severity": "medium",
				"risk_tier": "medium",
				"summary": "Accepted current repair blocker.",
				"evidence": ["The accepted finding applies to the current lane head."],
				"resolver": "agent",
				"next_action": "Repair the accepted finding before handoff.",
				"finding_source": "accepted_findings",
				"finding_index": 0
			}]
		}),
	);

	assert!(!response.success);

	match &response.content_items[..] {
		[DynamicToolContentItem::InputText { text }] => {
			assert!(text.contains("full review is required"));
			assert!(text.contains("accepted_findings_present"));
			assert!(text.contains("blocking_finding_routes_present"));
			assert!(text.contains("nonclean_review_status"));
		},
		other => panic!("unexpected response content: {other:?}"),
	}
}

#[test]
fn compact_review_checkpoint_fails_closed_after_prior_nonclean_round() {
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
		review_context,
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let findings_response = review_policy::submit_findings_review_checkpoint(
		&bridge,
		"first full review found a current blocker",
	);

	assert!(findings_response.success);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "clean",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": review_policy::low_risk_handoff_review_contract_json(),
			"review_cost_control": review_policy::compact_review_cost_control_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer confirmed the accepted finding was repaired"]
		}),
	);

	assert!(!response.success);

	match &response.content_items[..] {
		[DynamicToolContentItem::InputText { text }] => {
			assert!(text.contains("prior_nonclean_review_rounds_present"));
		},
		other => panic!("unexpected response content: {other:?}"),
	}
}

#[test]
fn compact_review_checkpoint_fails_closed_after_prior_nonclean_round_on_repaired_head() {
	let tracker = FakeTracker::new();
	let issue = tests::sample_issue();
	let workflow = tests::sample_workflow();
	let temp_dir = TempDir::new().expect("tempdir should create");
	let review_context = tests::sample_review_context_in(temp_dir.path());
	let pull_request_inspector = FakePullRequestInspector::new(Vec::new());
	let mut repaired_local_repo = tests::sample_local_repo();

	repaired_local_repo.head_oid = String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	repaired_local_repo.head_tree_oid = String::from("28a20f7dfb9526e7421a5f095b1c6adec84e52d6");

	let repaired_head = repaired_local_repo.head_oid.clone();
	let local_repo_inspector =
		FakeLocalRepoInspector::new(vec![Ok(tests::sample_local_repo()), Ok(repaired_local_repo)]);
	let bridge = TrackerToolBridge::with_review_handoff_for_test(
		&tracker,
		&issue,
		&workflow,
		review_context,
		&pull_request_inspector,
		&local_repo_inspector,
	);
	let findings_response = review_policy::submit_findings_review_checkpoint(
		&bridge,
		"first full review found a current blocker",
	);

	assert!(findings_response.success);

	let response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "clean",
			"head_sha": repaired_head,
			"review_contract": review_policy::low_risk_handoff_review_contract_json(),
			"review_cost_control": review_policy::compact_review_cost_control_json(),
			"checks": review_policy::review_checks_json(),
			"evidence": ["fresh reviewer confirmed the accepted finding was repaired on a new head"]
		}),
	);

	assert!(!response.success);

	match &response.content_items[..] {
		[DynamicToolContentItem::InputText { text }] => {
			assert!(text.contains("prior_nonclean_review_rounds_present"));
		},
		other => panic!("unexpected response content: {other:?}"),
	}
}
