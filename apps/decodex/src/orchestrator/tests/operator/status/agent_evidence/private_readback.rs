use crate::orchestrator::tests::operator::status::{
	self, AuthorityBoundaryChangedSurface, AuthorityBoundaryCheckInput,
	AuthorityBoundaryDisposition, AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface,
	AuthorityDecisionRequestInput, EvidenceRequest, ReviewLevel, StateStore, TEST_SERVICE_ID,
	TempDir, fs, orchestrator,
};

#[test]
fn private_evidence_readback_summarizes_payloads_without_connector() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_worktree(TEST_SERVICE_ID, "issue-1", "x/pubfi-pub-101", ".worktrees/PUB-101")
		.expect("worktree should persist");
	state_store.record_run_attempt("run-1", "issue-1", 1, "failed").expect("run should persist");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-1",
			"run-1",
			1,
			"command_failed",
			serde_json::json!({
				"summary": "cargo make test failed",
				"next_action": "repair the failing assertion",
				"stdout": "full command output stays hidden by default",
			}),
		)
		.expect("private evidence should append");

	let request = EvidenceRequest {
		config_path: None,
		project_id: None,
		issue: "PUB-101",
		run_id: Some("run-1"),
		attempt_number: Some(1),
		json: true,
		include_payload: false,
	};
	let readback = orchestrator::build_private_evidence_readback(&state_store, &config, &request)
		.expect("private evidence should read from local state");

	assert_eq!(readback.event_count, 1);
	assert_eq!(readback.issue_id, "issue-1");
	assert_eq!(readback.issue_identifier.as_deref(), Some("PUB-101"));
	assert_eq!(readback.latest_event_type.as_deref(), Some("command_failed"));
	assert!(readback.warnings.is_empty());
	assert_eq!(readback.events[0].payload, None);
	assert!(
		readback.events[0]
			.payload_summary
			.preview
			.iter()
			.any(|preview| preview.contains("summary=cargo make test failed"))
	);
	assert_eq!(
		readback.events[0].payload_summary.redacted_default_keys,
		vec![String::from("stdout")]
	);

	let rendered = orchestrator::render_private_evidence_readback(&readback);

	assert!(rendered.contains("event_count: 1"));
	assert!(rendered.contains("redacted_default_keys=stdout"));
	assert!(!rendered.contains("full command output stays hidden by default"));
}

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

#[test]
fn private_evidence_readback_reports_missing_events_for_known_run() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_worktree(TEST_SERVICE_ID, "issue-2", "x/pubfi-pub-102", ".worktrees/PUB-102")
		.expect("worktree should persist");
	state_store
		.record_run_attempt("run-empty", "issue-2", 1, "running")
		.expect("run should persist");

	let request = EvidenceRequest {
		config_path: None,
		project_id: None,
		issue: "PUB-102",
		run_id: Some("run-empty"),
		attempt_number: Some(1),
		json: false,
		include_payload: false,
	};
	let readback = orchestrator::build_private_evidence_readback(&state_store, &config, &request)
		.expect("missing private evidence should still produce readback");

	assert_eq!(readback.event_count, 0);
	assert_eq!(readback.warnings, vec![String::from("private_execution_evidence_missing")]);
	assert!(orchestrator::render_private_evidence_readback(&readback).contains("- none"));
}

#[test]
fn private_evidence_readback_direct_lookup_uses_stored_issue_id() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-1",
			"run-detached",
			3,
			"progress_checkpoint",
			serde_json::json!({
				"summary": "private checkpoint stayed local",
			}),
		)
		.expect("private evidence should append without run metadata");

	let request = EvidenceRequest {
		config_path: None,
		project_id: None,
		issue: "PUB-101",
		run_id: Some("run-detached"),
		attempt_number: Some(3),
		json: true,
		include_payload: false,
	};
	let readback = orchestrator::build_private_evidence_readback(&state_store, &config, &request)
		.expect("direct private evidence lookup should infer stored issue id");

	assert_eq!(readback.event_count, 1);
	assert_eq!(readback.issue_id, "issue-1");
	assert_eq!(readback.issue_identifier.as_deref(), Some("PUB-101"));
	assert_eq!(readback.latest_event_type.as_deref(), Some("progress_checkpoint"));
	assert!(readback.warnings.is_empty());
}

#[test]
fn private_evidence_readback_returns_manual_adopt_events_and_stable_command() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "pub-1579-manual-adopt-2-de5a4c6bf98a";

	state_store
		.record_run_attempt(run_id, "issue-pub-1579", 2, "succeeded")
		.expect("manual adopt run should persist");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-pub-1579",
			run_id,
			2,
			"review_handoff_adopt",
			serde_json::json!({
				"schema": "decodex.review_handoff_recovery_private_event/1",
				"event": "review_handoff_adopt",
				"writeback_stage": "active_label_checked",
				"issue_identifier": "PUB-1579",
				"branch": "x/pubfi-mono-pub-1579",
				"worktree_path": ".worktrees/PUB-1579",
				"pr_url": "https://github.com/helixbox/pubfi-mono/pull/340",
				"pr_head_sha": "4080f5e000000000000000000000000000000000",
				"pr_base_ref": "main",
				"pr_state": "OPEN",
				"mergeable": "MERGEABLE",
				"merge_state_status": "CLEAN",
				"status_check_rollup_state": "SUCCESS",
				"active_label_present": false,
				"active_label_restored": true,
				"existing_retained_worktree_mapping": true,
				"existing_review_handoff_marker": false,
				"manual_takeover_adopt": true,
				"next_action": "continue retained post-review lifecycle"
			}),
		)
		.expect("manual adopt private evidence should append");

	let request = EvidenceRequest {
		config_path: None,
		project_id: None,
		issue: "PUB-1579",
		run_id: Some(run_id),
		attempt_number: Some(2),
		json: true,
		include_payload: false,
	};
	let readback = orchestrator::build_private_evidence_readback(&state_store, &config, &request)
		.expect("manual adopt evidence should read");
	let expected_command = format!(
		"decodex evidence --config {} PUB-1579 --run-id {run_id} --attempt 2 --json",
		config.config_path().display()
	);

	assert_eq!(readback.event_count, 1);
	assert_eq!(readback.issue_id, "issue-pub-1579");
	assert_eq!(readback.issue_identifier.as_deref(), Some("PUB-1579"));
	assert_eq!(readback.latest_event_type.as_deref(), Some("review_handoff_adopt"));
	assert_eq!(readback.read_command, expected_command);
	assert!(readback.warnings.is_empty());
}

#[test]
fn private_evidence_readback_shell_quotes_config_paths_with_spaces() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = temp_dir.path().join("parent space/target-repo");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "pub-space-attempt-1";

	fs::create_dir_all(&repo_root).expect("repo root should exist");
	status::write_service_config(
		&repo_root,
		&status::sample_service_config_toml("pubfi", "HOME", "HOME", None, ReviewLevel::Strict),
	);

	let config = status::load_service_config(&repo_root);

	state_store.record_run_attempt(run_id, "issue-space", 1, "failed").expect("run should persist");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-space",
			run_id,
			1,
			"harness_outcome",
			serde_json::json!({"schema": "decodex.harness_outcome/1"}),
		)
		.expect("private evidence should append");

	let request = EvidenceRequest {
		config_path: None,
		project_id: None,
		issue: "issue-space",
		run_id: Some(run_id),
		attempt_number: Some(1),
		json: true,
		include_payload: false,
	};
	let readback = orchestrator::build_private_evidence_readback(&state_store, &config, &request)
		.expect("private evidence should read");

	assert_eq!(
		readback.read_command,
		format!(
			"decodex evidence --config '{}' issue-space --run-id {run_id} --attempt 1 --json",
			config.config_path().display()
		)
	);
}
