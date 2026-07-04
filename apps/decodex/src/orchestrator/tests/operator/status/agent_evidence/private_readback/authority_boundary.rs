use crate::orchestrator::tests::operator::status::{
	self, AuthorityBoundaryChangedSurface, AuthorityBoundaryCheckInput,
	AuthorityBoundaryDisposition, AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface,
	AuthorityDecisionRequestInput, EvidenceRequest, StateStore, TEST_SERVICE_ID, orchestrator,
};

#[test]
fn agent_evidence_authority_boundary_readback_recommends_candidates_without_payload_leakage() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let private_marker = "PRIVATE_AUTHORITY_READBACK_PAYLOAD";

	state_store
		.upsert_worktree(TEST_SERVICE_ID, "issue-boundary", "x/pubfi-pub-111", ".worktrees/PUB-111")
		.expect("worktree should persist");
	state_store
		.record_run_attempt("run-boundary", "issue-boundary", 1, "terminal_guarded")
		.expect("run should persist");

	orchestrator::record_authority_boundary_check_private_event(
		&state_store,
		AuthorityBoundaryCheckInput {
			project_id: TEST_SERVICE_ID,
			issue_id: "issue-boundary",
			issue_identifier: "PUB-111",
			run_id: "run-boundary",
			attempt_number: 1,
			decision_contract_ids: vec!["contract-boundary"],
			attempted_recovery_reason: "ambiguous_retained_progress",
			changed_surfaces: vec![AuthorityBoundaryChangedSurface {
				surface: AuthorityBoundarySurface::RetainedOwnership,
				change_summary: private_marker,
				policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
				legacy_disposition: AuthorityBoundaryDisposition::RequiresHuman,
			}],
			policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			final_disposition_reason: "Authority evidence is underspecified for recovery.",
			improvement_signals: vec![
				orchestrator::AuthorityBoundaryImprovementSignal {
					kind: "underspecified_decision_contract",
					reason_code: "authority_underspecified",
					target: "decision_contract:contract-boundary",
					recommendation: "Record validation-gate authority before recovery.",
				},
				orchestrator::AuthorityBoundaryImprovementSignal {
					kind: "missing_issue_template_field",
					reason_code: "authority_boundary_template_gap",
					target: "issue_template:loop_recovery",
					recommendation: "Add changed-surface prompts to the issue template.",
				},
			],
		},
	)
	.expect("authority boundary check should persist");

	let request = EvidenceRequest {
		config_path: None,
		project_id: None,
		issue: "PUB-111",
		run_id: Some("run-boundary"),
		attempt_number: Some(1),
		json: true,
		include_payload: false,
	};
	let readback = orchestrator::build_private_evidence_readback(&state_store, &config, &request)
		.expect("authority boundary evidence should read");
	let rendered = orchestrator::render_private_evidence_readback(&readback);

	assert_eq!(readback.event_count, 1);
	assert_eq!(readback.latest_event_type.as_deref(), Some("authority_boundary_check"));
	assert!(readback.events.iter().all(|event| event.payload.is_none()));
	assert!(readback.improvement_candidates.iter().any(|candidate| {
		candidate.kind == "underspecified_decision_contract"
			&& candidate.reason_code == "authority_underspecified"
			&& candidate.target == "decision_contract:contract-boundary"
	}));
	assert!(readback.improvement_candidates.iter().any(|candidate| {
		candidate.kind == "missing_issue_template_field"
			&& candidate.reason_code == "authority_boundary_template_gap"
	}));
	assert!(rendered.contains("authority_underspecified"));
	assert!(!rendered.contains(private_marker));
}

#[test]
fn agent_evidence_private_readback_summarizes_authority_decision_request_without_payload_leakage() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let private_diff_evidence = "PRIVATE_DECISION_REQUEST_DIFF_PAYLOAD";

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			"issue-decision-request",
			"x/pubfi-pub-112",
			".worktrees/PUB-112",
		)
		.expect("worktree should persist");
	state_store
		.record_run_attempt("run-decision-request", "issue-decision-request", 1, "terminal_guarded")
		.expect("run should persist");

	let boundary_event = orchestrator::record_authority_boundary_check_private_event(
		&state_store,
		AuthorityBoundaryCheckInput {
			project_id: TEST_SERVICE_ID,
			issue_id: "issue-decision-request",
			issue_identifier: "PUB-112",
			run_id: "run-decision-request",
			attempt_number: 1,
			decision_contract_ids: vec!["contract-decision-request"],
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
	.expect("authority boundary check should persist");

	orchestrator::record_authority_decision_request_private_event(
		&state_store,
		AuthorityDecisionRequestInput {
			project_id: TEST_SERVICE_ID,
			issue_id: "issue-decision-request",
			issue_identifier: "PUB-112",
			run_id: "run-decision-request",
			attempt_number: 1,
			boundary_check_record_id: boundary_event.record_id(),
			decision_request_id: "dr-pub-112-1",
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
			retained_diff_evidence: vec![private_diff_evidence],
			recovery_attempt_context: vec!["recovery stopped at the authority boundary"],
		},
	)
	.expect("authority decision request should persist");

	let request = EvidenceRequest {
		config_path: None,
		project_id: None,
		issue: "PUB-112",
		run_id: Some("run-decision-request"),
		attempt_number: Some(1),
		json: true,
		include_payload: false,
	};
	let readback = orchestrator::build_private_evidence_readback(&state_store, &config, &request)
		.expect("decision request evidence should read");
	let rendered = orchestrator::render_private_evidence_readback(&readback);

	assert_eq!(readback.decision_requests.len(), 1);
	assert_eq!(readback.decision_requests[0].decision_request_id, "dr-pub-112-1");
	assert_eq!(readback.decision_requests[0].phase, "human_required");
	assert_eq!(readback.decision_requests[0].reason, "contract_boundary_required");
	assert!(rendered.contains("Decision Requests"));
	assert!(rendered.contains("dr-pub-112-1"));
	assert!(!rendered.contains(private_diff_evidence));
	assert!(readback.events.iter().all(|event| event.payload.is_none()));
}
