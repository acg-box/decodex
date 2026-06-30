use super::*;

use orchestrator::{HarnessOutcomeKind, HarnessOutcomeRecordInput, PrivateEvidenceReadback};

#[test]
fn agent_evidence_snapshot_writes_index_blockers_capsules_and_event_stream() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("temp path should be utf-8"));
	let mut current_lane = operator_status_text_current_lane();

	current_lane.suspected_stall = true;
	current_lane.phase = String::from("stalled");
	current_lane.run_phase = String::from("stalled");
	current_lane.counts_as_running = false;
	current_lane.needs_attention = true;

	let snapshot = OperatorStatusSnapshot {
		project_id: String::from(TEST_SERVICE_ID),
		run_limit: 10,
		status_source: None,
		snapshot_age_seconds: None,
		warnings: Vec::new(),
		warning_details: Vec::new(),
		connector_backoffs: Vec::new(),
		projects: vec![agent_evidence_project_status_with_configured_gh()],
		account_control: OperatorCodexAccountControlStatus {
			mode: String::from("balanced"),
			account_selector: None,
		},
		accounts: Vec::new(),
		current_lanes: vec![current_lane.clone()],
		recent_runs: vec![current_lane],
		history_lanes: Vec::new(),
		execution_programs: Vec::new(),
		queued_candidates: vec![agent_evidence_blocked_candidate()],
		worktrees: operator_status_text_worktrees(),
		post_review_lanes: vec![agent_evidence_missing_handoff_lane()],
	};
	let results = orchestrator::write_agent_evidence_snapshot(
		&snapshot,
		AgentEvidenceSource::DiagnoseCommand,
	)
	.expect("agent evidence should write");
	let result = results.first().expect("project evidence should exist");
	let index_path = temp_dir.path().join(".codex/decodex/agent-evidence/pubfi/handoff-index.json");
	let index_json = read_json_file(&index_path);

	assert_eq!(result.project_id, TEST_SERVICE_ID);
	assert_eq!(result.handoff_index_path, index_path.display().to_string());
	assert_eq!(index_json["schema"], "decodex.agent_handoff_index/1");
	assert_eq!(index_json["project_id"], TEST_SERVICE_ID);
	assert_eq!(index_json["source"], "diagnose_command");

	assert_agent_evidence_github_cli_authority(&index_json);

	assert_eq!(index_json["summary"]["blocker_count"], 3);
	assert_eq!(index_json["summary"]["run_capsule_count"], 1);
	assert_eq!(
		index_json["run_capsules"][0]["private_evidence"]["evidence_ref"],
		"private-evidence:pubfi/issue-1/run-1/1"
	);
	assert_eq!(
		index_json["run_capsules"][0]["private_evidence"]["read_command"],
		"decodex evidence --config project.toml PUB-101 --run-id run-1 --attempt 1 --json"
	);
	assert_eq!(
		index_json["blockers"][0]["blocker_snapshot_path"],
		temp_dir
			.path()
			.join(".codex/decodex/agent-evidence/pubfi/blockers/pub-101.json")
			.display()
			.to_string()
	);
	assert!(
		index_json["recovery_contracts"]
			.as_array()
			.expect("recovery contracts should be array")
			.iter()
			.any(|contract| contract["reason_code"] == "missing_review_handoff_record")
	);

	let capsule_path = index_json["run_capsules"][0]["path"]
		.as_str()
		.expect("run capsule path should be a string");
	let capsule_json = read_json_file(Path::new(capsule_path));

	assert_eq!(capsule_json["schema"], "decodex.run_capsule/1");
	assert_eq!(capsule_json["run_id"], "run-1");
	assert_eq!(capsule_json["diagnosis"]["reason_code"], "suspected_stall");
	assert_eq!(capsule_json["private_evidence"]["default_view"], "summarized_payloads");

	let blocker_json = read_json_file(
		&temp_dir.path().join(".codex/decodex/agent-evidence/pubfi/blockers/pub-101.json"),
	);

	assert_eq!(blocker_json["schema"], "decodex.blocker_snapshot/1");
	assert_eq!(blocker_json["issue_identifier"], "PUB-101");
	assert_eq!(blocker_json["related_run_capsules"][0]["run_id"], "run-1");

	let events_path = temp_dir.path().join(".codex/decodex/agent-evidence/pubfi/events.jsonl");
	let events_body = fs::read_to_string(events_path).expect("events stream should exist");
	let event_json: Value =
		serde_json::from_str(events_body.lines().next().expect("event line should exist"))
			.expect("event should be JSON");

	assert_eq!(event_json["schema"], "decodex.agent_evidence_event/1");
	assert_eq!(event_json["blocker_count"], 3);
}

#[test]
fn agent_evidence_snapshot_does_not_turn_waiting_running_lane_into_attention_blocker() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("temp path should be utf-8"));
	let mut current_lane = operator_status_text_current_lane();

	current_lane.wait_reason = Some(String::from("tool_execution"));
	current_lane.lane_control_next_action = String::from("continue_owned_attempt");
	current_lane.counts_as_running = true;
	current_lane.needs_attention = false;
	current_lane.suspected_stall = false;

	let snapshot = OperatorStatusSnapshot {
		project_id: String::from(TEST_SERVICE_ID),
		run_limit: 10,
		status_source: None,
		snapshot_age_seconds: None,
		warnings: Vec::new(),
		warning_details: Vec::new(),
		connector_backoffs: Vec::new(),
		projects: vec![agent_evidence_project_status_with_configured_gh()],
		account_control: OperatorCodexAccountControlStatus {
			mode: String::from("balanced"),
			account_selector: None,
		},
		accounts: Vec::new(),
		current_lanes: vec![current_lane.clone()],
		recent_runs: vec![current_lane],
		history_lanes: Vec::new(),
		execution_programs: Vec::new(),
		queued_candidates: Vec::new(),
		worktrees: operator_status_text_worktrees(),
		post_review_lanes: Vec::new(),
	};
	let results =
		orchestrator::write_agent_evidence_snapshot(&snapshot, AgentEvidenceSource::ServeTick)
			.expect("agent evidence should write");
	let result = results.first().expect("project evidence should exist");
	let index_path = temp_dir.path().join(".codex/decodex/agent-evidence/pubfi/handoff-index.json");
	let index_json = read_json_file(&index_path);

	assert_eq!(result.handoff_index.summary.blocker_count, 0);
	assert_eq!(index_json["summary"]["blocker_count"], 0);
	assert_eq!(index_json["blockers"].as_array().expect("blockers should be array").len(), 0);

	let capsule_path = index_json["run_capsules"][0]["path"]
		.as_str()
		.expect("run capsule path should be a string");
	let capsule_json = read_json_file(Path::new(capsule_path));

	assert_eq!(capsule_json["wait_reason"], "tool_execution");
	assert_eq!(capsule_json["diagnosis"]["attention_required"], false);
	assert!(capsule_json["diagnosis"]["reason_code"].is_null());
	assert!(
		!temp_dir.path().join(".codex/decodex/agent-evidence/pubfi/blockers/pub-101.json").exists(),
		"ordinary wait_reason must not create a stale attention blocker file"
	);
}

fn assert_agent_evidence_github_cli_authority(index_json: &Value) {
	assert_eq!(index_json["github_cli_authority"]["discovery_tier"], "configured");
	assert_eq!(index_json["github_cli_authority"]["command_path"], "/opt/homebrew/bin/gh");
	assert_eq!(
		index_json["github_cli_authority"]["next_action"],
		"No action needed; Decodex will use the configured GitHub CLI path."
	);
}

fn agent_evidence_blocked_candidate() -> OperatorQueuedIssueStatus {
	let mut blocked_candidate = operator_status_text_queued_candidates()
		.into_iter()
		.find(|candidate| candidate.issue_identifier == "PUB-102")
		.expect("fixture should include queued issue");

	blocked_candidate.classification = String::from("blocked");
	blocked_candidate.reason = String::from("missing_dispatch_briefing");

	blocked_candidate
}

fn agent_evidence_missing_handoff_lane() -> OperatorPostReviewLaneStatus {
	let mut missing_handoff_lane = operator_status_text_post_review_lanes()
		.into_iter()
		.next()
		.expect("fixture should include retained review lane");

	missing_handoff_lane.classification = String::from("blocked");
	missing_handoff_lane.reason = String::from("missing_review_handoff_record");

	missing_handoff_lane
}

fn agent_evidence_project_status_with_configured_gh() -> OperatorProjectStatus {
	OperatorProjectStatus {
		project_id: String::from(TEST_SERVICE_ID),
		config_path: String::from("project.toml"),
		repo_root: String::from("/repo/pubfi"),
		enabled: true,
		github_cli_authority: OperatorGitHubCliAuthority {
			command_path: String::from("/opt/homebrew/bin/gh"),
			resolved_path: Some(String::from("/opt/homebrew/bin/gh")),
			configured_path: Some(String::from("/opt/homebrew/bin/gh")),
			discovery_tier: String::from("configured"),
			available: true,
			next_action: String::from(
				"No action needed; Decodex will use the configured GitHub CLI path.",
			),
		},
		current_lane_count: 0,
		running_lane_count: 0,
		queued_candidate_count: 0,
		post_review_lane_count: 0,
		retained_worktree_count: 0,
		waiting_lane_count: 0,
		attention_count: 0,
		cleanup_blocked_count: 0,
		cleanup_pending_count: 0,
		connector_state: String::from("ok"),
		last_activity_at: None,
		warning_count: 0,
	}
}

#[test]
fn private_evidence_readback_summarizes_payloads_without_connector() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
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
	let (_temp_dir, config, _workflow) = temp_project_layout();
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
	let (_temp_dir, config, _workflow) = temp_project_layout();
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
	let (_temp_dir, config, _workflow) = temp_project_layout();
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
	let (_temp_dir, config, _workflow) = temp_project_layout();
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
	let (_temp_dir, config, _workflow) = temp_project_layout();
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

	write_service_config(
		&repo_root,
		&sample_service_config_toml("pubfi", "HOME", "HOME", None, ReviewLevel::Strict),
	);

	let config = load_service_config(&repo_root);

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

#[test]
fn agent_evidence_harness_outcome_records_validation_review_and_repair_signals() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.record_run_attempt("run-harness", "issue-harness", 2, "failed")
		.expect("run should persist");

	record_harness_signal_fixture_events(&state_store);

	let recorded = orchestrator::record_harness_outcome_for_issue_run(
		&state_store,
		HarnessOutcomeRecordInput {
			project_id: TEST_SERVICE_ID,
			issue_id: "issue-harness",
			issue_identifier: "PUB-110",
			run_id: "run-harness",
			attempt_number: 2,
			outcome: HarnessOutcomeKind::RetryableFailure,
			error_class: Some("repo_gate_verify_failed"),
			validation_result: Some("failed"),
			pr_url: None,
		},
	)
	.expect("harness outcome should record");
	let payload = recorded.payload();

	assert_eq!(recorded.event_type(), "harness_outcome");

	assert_harness_outcome_payload(payload);

	let request = EvidenceRequest {
		config_path: None,
		project_id: None,
		issue: "issue-harness",
		run_id: Some("run-harness"),
		attempt_number: Some(2),
		json: true,
		include_payload: false,
	};
	let readback = orchestrator::build_private_evidence_readback(&state_store, &config, &request)
		.expect("private evidence should read");

	assert_harness_private_readback(&readback);
}

#[test]
fn harness_outcome_does_not_report_validation_failed_for_review_no_effective_diff() {
	let (_temp_dir, _config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.record_run_attempt("run-review-stalled", "issue-review-stalled", 1, "failed")
		.expect("run should persist");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-review-stalled",
			"run-review-stalled",
			1,
			"review_checkpoint",
			serde_json::json!({
				"phase": "handoff",
				"status": "findings",
				"head_sha": "abc123",
				"nonclean_rounds": 2,
				"review": {
					"accepted_findings": [
						{"summary": "remove stale storage docs"},
						{"summary": "update runbook handoff evidence"}
					],
					"rejected_findings": []
				}
			}),
		)
		.expect("review checkpoint should append");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-review-stalled",
			"run-review-stalled",
			1,
			"loop_guardrail_checkpoint",
			serde_json::json!({
				"schema": "decodex.loop_guardrail_checkpoint/1",
				"reason": "no_effective_diff",
				"consecutive_count": 1,
				"threshold": 3,
				"source_error_class": null,
				"details": {
					"effective_delta_present": false
				}
			}),
		)
		.expect("guardrail checkpoint should append");

	let recorded = orchestrator::record_harness_outcome_for_issue_run(
		&state_store,
		HarnessOutcomeRecordInput {
			project_id: TEST_SERVICE_ID,
			issue_id: "issue-review-stalled",
			issue_identifier: "PUB-1579",
			run_id: "run-review-stalled",
			attempt_number: 1,
			outcome: HarnessOutcomeKind::RetryableFailure,
			error_class: Some("retryable_execution_failure"),
			validation_result: None,
			pr_url: None,
		},
	)
	.expect("harness outcome should record");
	let payload = recorded.payload();

	assert_eq!(payload["validation"]["result"], "not_recorded");
	assert_eq!(payload["validation"]["failure_count"], 0);
	assert_eq!(payload["validation"]["failure_classes"], serde_json::json!([]));
	assert_eq!(payload["manual_attention"], Value::Null);

	assert_harness_payload_candidate(payload, "review_repair_no_effective_diff_after_findings");
}

fn assert_harness_outcome_payload(payload: &Value) {
	assert_eq!(payload["schema"], "decodex.harness_outcome/1");
	assert_eq!(payload["validation"]["result"], "failed");
	assert_eq!(payload["validation"]["failure_count"], 2);
	assert_eq!(
		payload["validation"]["failure_classes"],
		serde_json::json!(["phase_goal_validation_fail", "repo_gate_verify_failed"])
	);
	assert_eq!(payload["repair"]["repair_attempt_observed"], true);
	assert_eq!(payload["review"]["accepted_finding_count"], 1);
	assert_eq!(payload["authority_boundary"]["failed_check_count"], 1);
	assert_eq!(payload["authority_boundary"]["improvement_signal_count"], 1);

	assert_harness_payload_candidate(payload, "accepted_review_findings");
	assert_harness_payload_candidate(payload, "authority_underspecified");
	assert_harness_payload_candidate(payload, "architecture_recovery_exhausted");
}

fn assert_harness_payload_candidate(payload: &Value, reason_code: &str) {
	assert!(
		payload["improvement_candidates"]
			.as_array()
			.expect("candidates should be an array")
			.iter()
			.any(|candidate| candidate["reason_code"] == reason_code)
	);
}

fn assert_harness_private_readback(readback: &PrivateEvidenceReadback) {
	assert!(
		readback
			.improvement_candidates
			.iter()
			.any(|candidate| candidate.reason_code == "accepted_review_findings")
	);
	assert!(
		readback
			.improvement_candidates
			.iter()
			.any(|candidate| candidate.reason_code == "authority_underspecified")
	);
	assert!(
		readback
			.improvement_candidates
			.iter()
			.any(|candidate| candidate.reason_code == "architecture_recovery_exhausted")
	);
	assert!(readback.architecture_recoveries.iter().any(|recovery| {
		recovery.reason_code == "architecture_recovery_exhausted"
			&& recovery.guardrail_reason.as_deref() == Some("validation_repeat")
			&& recovery.boundary_disposition.as_deref() == Some("within_authority")
			&& recovery.recovery_budget_attempt == Some(2)
			&& recovery.recovery_budget_max_attempts == Some(1)
	}));
	assert!(readback.review_checkpoints.iter().any(|checkpoint| {
		checkpoint.phase == "handoff"
			&& checkpoint.status == "findings"
			&& checkpoint.head_sha.as_deref() == Some("abc123")
			&& checkpoint.round == Some(1)
			&& checkpoint.review_class.as_deref() == Some("full_current_head_review")
			&& checkpoint.risk_class.as_deref() == Some("localized")
			&& checkpoint.compact_eligible == Some(false)
			&& checkpoint.fallback_reason.as_deref() == Some("accepted_findings_present")
			&& checkpoint.accepted_finding_count == 1
			&& checkpoint
				.route_counts
				.iter()
				.any(|count| count.route == "current_blocker" && count.count == 1)
			&& checkpoint.route_next_action.as_deref() == Some("Repair the accepted finding.")
	}));
	assert!(readback.phase_acceptance_checks.iter().any(|check| {
		check.phase == "implement_to_validation_ready"
			&& check.decision == "fail"
			&& check.reason_code == "no_effective_delta"
			&& !check.effective_delta_present
	}));
	assert!(readback.boundary_checks.iter().any(|boundary| {
		boundary.disposition == "requires_human"
			&& boundary.attempted_recovery_reason.as_deref() == Some("uncovered_direction")
			&& boundary.changed_surface_count == 1
			&& boundary.improvement_signal_count == 1
	}));

	let rendered = orchestrator::render_private_evidence_readback(readback);

	assert!(rendered.contains("Review Checkpoints"));
	assert!(rendered.contains("review_class: full_current_head_review"));
	assert!(rendered.contains("compact_eligible: false"));
	assert!(rendered.contains("review_fallback_reason: accepted_findings_present"));
	assert!(rendered.contains("route_counts: current_blocker=1"));
	assert!(rendered.contains("route_next_action: Repair the accepted finding."));
	assert!(rendered.contains("Phase Acceptance Checks"));
	assert!(rendered.contains("Architecture Recoveries"));
	assert!(rendered.contains("Boundary Checks"));
	assert!(rendered.contains("status: findings"));
	assert!(rendered.contains("reason_code: no_effective_delta"));
	assert!(rendered.contains("disposition: requires_human"));
	assert!(readback.events.iter().all(|event| event.payload.is_none()));
}

fn record_harness_signal_fixture_events(state_store: &StateStore) {
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-harness",
			"run-harness",
			2,
			"phase_goal_completed",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "repair_validation_failures",
				"payload": {
					"signal": "validation_fail",
					"status": "complete"
				}
			}),
		)
		.expect("phase goal evidence should append");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-harness",
			"run-harness",
			2,
			"review_checkpoint",
			serde_json::json!({
				"phase": "handoff",
				"status": "findings",
				"head_sha": "abc123",
				"nonclean_rounds": 1,
				"route_counts": [{"route": "current_blocker", "count": 1}],
				"route_next_action": "Repair the accepted finding.",
				"review_class": "full_current_head_review",
				"risk_class": "localized",
				"compact_eligible": false,
				"review_fallback_reason": "accepted_findings_present",
				"review": {
					"review_cost_control": {
						"review_class": "full_current_head_review",
						"risk_class": "localized",
						"compact_eligible": false,
						"fallback_reason": "accepted_findings_present"
					},
					"accepted_findings": [{"summary": "cover the missing edge case"}],
					"rejected_findings": [],
					"finding_route_summary": {
						"route_counts": [{"route": "current_blocker", "count": 1}],
						"next_action": "Repair the accepted finding."
					}
				}
			}),
		)
		.expect("review evidence should append");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-harness",
			"run-harness",
			2,
			"progress_checkpoint",
			serde_json::json!({
				"phase": "review_repair",
				"focus": "repair accepted finding"
			}),
		)
		.expect("progress evidence should append");

	record_harness_phase_acceptance_event(state_store);

	orchestrator::record_authority_boundary_check_private_event(
		state_store,
		AuthorityBoundaryCheckInput {
			project_id: TEST_SERVICE_ID,
			issue_id: "issue-harness",
			issue_identifier: "PUB-110",
			run_id: "run-harness",
			attempt_number: 2,
			decision_contract_ids: vec!["contract-harness"],
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
			improvement_signals: vec![orchestrator::AuthorityBoundaryImprovementSignal {
				kind: "underspecified_decision_contract",
				reason_code: "authority_underspecified",
				target: "decision_contract:contract-harness",
				recommendation: "Record accepted-behavior authority before recovery.",
			}],
		},
	)
	.expect("authority boundary evidence should append");

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-harness",
			"run-harness",
			2,
			"architecture_recovery_terminal",
			serde_json::json!({
				"schema": "decodex.architecture_recovery_terminal/1",
				"record_version": 1,
				"reason_code": "architecture_recovery_exhausted",
				"guardrail_reason": "validation_repeat",
				"boundary_disposition": "within_authority",
				"recovery_budget": {
					"attempt": 2,
					"max_attempts": 1,
				},
			}),
		)
		.expect("architecture recovery evidence should append");
}

fn record_harness_phase_acceptance_event(state_store: &StateStore) {
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			"issue-harness",
			"run-harness",
			2,
			PHASE_ACCEPTANCE_CHECK_EVENT_TYPE,
			serde_json::json!({
				"schema": "decodex.phase_acceptance_check/1",
				"phase": "implement_to_validation_ready",
				"decision": "fail",
				"reason_code": "no_effective_delta",
				"objective_coverage": { "covered": true },
				"effective_delta": {
					"present": false,
					"changed_surfaces": ["runtime"],
				},
				"non_goal_check": {
					"passed": true,
					"blocker_count": 0,
				},
				"validation_evidence": {
					"repo_gate_passed": true,
				},
				"next_action": "produce an issue-scoped effective delta before completing the phase goal again",
			}),
		)
		.expect("phase acceptance evidence should append");
}

#[test]
fn harness_eval_fixture_recommends_contract_improvement_without_payload_leakage() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let fixture: Value = serde_json::from_str(include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/fixtures/harness_improvement/incomplete_contract_eval.json"
	)))
	.expect("harness eval fixture should parse");
	let issue_id = fixture["issue_id"].as_str().expect("fixture issue id");
	let issue_identifier = fixture["issue_identifier"].as_str().expect("fixture issue identifier");
	let run_id = fixture["run_id"].as_str().expect("fixture run id");
	let attempt_number = fixture["attempt_number"].as_i64().expect("fixture attempt");
	let contract: DecisionContract = serde_json::from_value(fixture["decision_contract"].clone())
		.expect("fixture contract should deserialize");

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			issue_id,
			"y/decodex-xy-857-eval",
			".worktrees/XY-857-EVAL",
		)
		.expect("worktree should persist");
	state_store
		.record_run_attempt(run_id, issue_id, attempt_number, "terminal_guarded")
		.expect("run should persist");
	state_store
		.upsert_decision_contract(TEST_SERVICE_ID, Some(issue_id), contract)
		.expect("contract should persist");

	for event in fixture["private_events"].as_array().expect("fixture events") {
		state_store
			.append_private_execution_event(
				TEST_SERVICE_ID,
				issue_id,
				run_id,
				attempt_number,
				event["event_type"].as_str().expect("fixture event type"),
				event["payload"].clone(),
			)
			.expect("fixture event should append");
	}

	orchestrator::record_harness_outcome_for_issue_run(
		&state_store,
		HarnessOutcomeRecordInput {
			project_id: TEST_SERVICE_ID,
			issue_id,
			issue_identifier,
			run_id,
			attempt_number,
			outcome: HarnessOutcomeKind::ManualAttention,
			error_class: Some("uncovered_direction"),
			validation_result: None,
			pr_url: None,
		},
	)
	.expect("harness outcome should record");

	let request = EvidenceRequest {
		config_path: None,
		project_id: None,
		issue: issue_identifier,
		run_id: Some(run_id),
		attempt_number: Some(attempt_number),
		json: false,
		include_payload: false,
	};
	let readback = orchestrator::build_private_evidence_readback(&state_store, &config, &request)
		.expect("fixture readback should summarize private evidence");
	let expected = &fixture["expected_candidate"];

	assert!(readback.improvement_candidates.iter().any(|candidate| {
		candidate.kind == expected["kind"].as_str().expect("expected kind")
			&& candidate.reason_code == expected["reason_code"].as_str().expect("expected reason")
			&& candidate.target == expected["target"].as_str().expect("expected target")
	}));

	let rendered = orchestrator::render_private_evidence_readback(&readback);

	assert!(rendered.contains("Improvement Candidates"));
	assert!(rendered.contains("underspecified_decision_contract"));
	assert!(!rendered.contains("Decide how generated issues must cite"));
	assert!(readback.events.iter().all(|event| event.payload.is_none()));
}

fn read_json_file(path: &Path) -> Value {
	let body = fs::read_to_string(path).expect("JSON file should exist");

	serde_json::from_str(&body).expect("JSON file should parse")
}
