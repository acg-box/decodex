use crate::orchestrator::tests::operator::status::{
	self, AuthorityBoundaryChangedSurface, AuthorityBoundaryCheckInput,
	AuthorityBoundaryDisposition, AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface,
	AuthorityDecisionRequestInput, FakeTracker, StateStore, TEST_SERVICE_ID, orchestrator,
};

#[test]
fn live_operator_status_snapshot_surfaces_authority_decision_request() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-decision-request",
		"PUB-118",
		"Todo",
		&["decodex:needs-attention"],
		Some(2),
		"2026-03-13T09:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	tracker.issue_comments.borrow_mut().insert(
		issue.id.clone(),
		vec![status::linear_execution_history_comment(
			&issue,
			"needs_attention",
			"2026-03-13T09:20:00Z",
			"contract-boundary-required",
			|record| {
				record.error_class = Some(String::from("contract_boundary_required"));
				record.next_action = Some(String::from(
					"accept, reject, or revise decision request `dr-pub-118-1`, then clear needs-attention and requeue through Decodex",
				));
				record.summary =
					Some(String::from("Authority boundary requires a human decision."));
				record.blockers = Some(vec![String::from(
					"accepted behavior change exceeds current authority",
				)]);
				record.evidence = Some(vec![String::from(
					"authority boundary check requires human direction",
				)]);
				record.terminal_path = Some(String::from("manual_attention"));
			},
		)],
	);

	let boundary_event = orchestrator::record_authority_boundary_check_private_event(
		&state_store,
		AuthorityBoundaryCheckInput {
			project_id: TEST_SERVICE_ID,
			issue_id: &issue.id,
			issue_identifier: &issue.identifier,
			run_id: "xy-355-attempt-1-1777527013",
			attempt_number: 1,
			decision_contract_ids: vec!["contract-pub-118"],
			attempted_recovery_reason: "uncovered_direction",
			changed_surfaces: vec![AuthorityBoundaryChangedSurface {
				surface: AuthorityBoundarySurface::Objective,
				change_summary: "Public behavior would change.",
				policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
				legacy_disposition: AuthorityBoundaryDisposition::RequiresHuman,
			}],
			policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			final_disposition_reason: "Accepted behavior needs explicit authority.",
			improvement_signals: Vec::new(),
		},
	)
	.expect("boundary event should persist");

	orchestrator::record_authority_decision_request_private_event(
		&state_store,
		AuthorityDecisionRequestInput {
			project_id: TEST_SERVICE_ID,
			issue_id: &issue.id,
			issue_identifier: &issue.identifier,
			run_id: "xy-355-attempt-1-1777527013",
			attempt_number: 1,
			boundary_check_record_id: boundary_event.record_id(),
			decision_request_id: "dr-pub-118-1",
			reason_code: "contract_boundary_required",
			boundary_type: "accepted_behavior",
			proposed_change: "Change accepted operator behavior.",
			why_exceeds_authority: "The current issue did not authorize the behavior change.",
			options: vec![orchestrator::AuthorityDecisionOption {
				label: "revise",
				description: "Update the Decision Contract before resuming.",
			}],
			recommendation: "Revise the Decision Contract before resuming automation.",
			resume_condition: "Clear needs-attention and requeue only after authority is updated.",
			retained_worktree_evidence: vec!["retained worktree has tracked changes"],
			retained_diff_evidence: vec!["private diff summary retained locally"],
			recovery_attempt_context: vec!["recovery stopped at the authority boundary"],
		},
	)
	.expect("decision request should persist");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let candidate = snapshot
		.queued_candidates
		.iter()
		.find(|candidate| candidate.issue_identifier == "PUB-118")
		.expect("needs-attention queued issue should exist");
	let decision_request = candidate
		.attention
		.as_ref()
		.and_then(|attention| attention.decision_request.as_ref())
		.expect("decision request should render");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(decision_request.phase, "human_required");
	assert_eq!(decision_request.reason, "contract_boundary_required");
	assert_eq!(decision_request.boundary, "accepted_behavior");
	assert_eq!(decision_request.decision_request_id, "dr-pub-118-1");
	assert!(rendered.contains("decision_request_phase: human_required"));
	assert!(rendered.contains("decision_request_id: dr-pub-118-1"));
}

#[test]
fn live_operator_status_snapshot_surfaces_plugin_list_preflight_timeout() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-needs-attention",
		"PUB-109",
		"Todo",
		&["decodex:needs-attention"],
		Some(2),
		"2026-03-13T09:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	tracker.issue_comments.borrow_mut().insert(
		issue.id.clone(),
		vec![status::linear_execution_history_comment(
			&issue,
			"terminal_failure",
			"2026-03-13T09:20:00Z",
			"app-server-plugin-list-timeout",
			|record| {
				record.error_class = Some(String::from("app_server_plugin_list_timeout"));
				record.next_action = Some(String::from(
					"inspect local app_server_preflight_failed evidence for the `plugin/list` timeout, restart `decodex serve`, run `decodex probe`, clear label `decodex:needs-attention`",
				));
				record.summary = Some(String::from("Decodex run failed and needs attention."));
				record.blockers = Some(vec![String::from(
					"plugin/list timed out during app-server preflight",
				)]);
				record.evidence = Some(vec![String::from(
					"app_server_preflight_failed happened before thread/start",
				)]);
			},
		)],
	);

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let candidate = snapshot
		.queued_candidates
		.iter()
		.find(|candidate| candidate.issue_identifier == "PUB-109")
		.expect("needs-attention queued issue should exist");
	let attention = candidate.attention.as_ref().expect("attention details should render");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(attention.attention_error_class.as_deref(), Some("app_server_plugin_list_timeout"));
	assert!(attention.summary.contains("app_server_preflight_failed: plugin/list timed out"));
	assert!(rendered.contains("attention_cause: app_server_plugin_list_timeout"));
	assert!(rendered.contains("attention_next_action: inspect local app_server_preflight_failed"));
	assert!(rendered.contains("plugin/list"));
}
