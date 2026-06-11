use state::ReviewPolicyCheckpointInput;

#[test]
fn operator_status_text_surfaces_github_cli_authority() {
	let snapshot = OperatorStatusSnapshot {
		project_id: String::from("pubfi"),
		run_limit: 10,
		status_source: None,
		snapshot_age_seconds: None,
		warnings: Vec::new(),
		warning_details: Vec::new(),
		connector_backoffs: Vec::new(),
		projects: vec![OperatorProjectStatus {
			project_id: String::from("pubfi"),
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
			active_run_count: 0,
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
		}],
		account_control: OperatorCodexAccountControlStatus {
			mode: String::from("balanced"),
			account_selector: None,
		},
		accounts: Vec::new(),
		active_runs: Vec::new(),
		queued_candidates: Vec::new(),
		recent_runs: Vec::new(),
		history_lanes: Vec::new(),
		execution_programs: Vec::new(),
		worktrees: Vec::new(),
		post_review_lanes: Vec::new(),
	};
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains(
		"GitHub CLI: tier=configured available=true command_path=/opt/homebrew/bin/gh resolved_path=/opt/homebrew/bin/gh configured_path=/opt/homebrew/bin/gh next_action=No action needed; Decodex will use the configured GitHub CLI path."
	));
}

#[test]
fn operator_status_text_renders_human_readable_sections() {
	let active_run = operator_status_text_active_run();
	let snapshot = OperatorStatusSnapshot {
		project_id: String::from("pubfi"),
		run_limit: 10,
		status_source: None,
		snapshot_age_seconds: None,
		warnings: Vec::new(),
		warning_details: Vec::new(),
		connector_backoffs: Vec::new(),
		projects: Vec::new(),
		account_control: OperatorCodexAccountControlStatus {
			mode: String::from("balanced"),
			account_selector: None,
		},
		accounts: Vec::new(),
		active_runs: vec![active_run.clone()],
		queued_candidates: operator_status_text_queued_candidates(),
		recent_runs: vec![active_run],
		history_lanes: Vec::new(),
		execution_programs: Vec::new(),
		worktrees: operator_status_text_worktrees(),
		post_review_lanes: operator_status_text_post_review_lanes(),
	};
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains("Project: pubfi"));
	assert!(rendered.contains("Warnings: 0"));
	assert!(rendered.contains("Running Lanes"));
	assert!(rendered.contains(
		"Run ledger shown: 0 issue lanes from 0 history attempts (running lanes inline)"
	));
	assert!(rendered.contains("Run Ledger"));
	assert!(rendered.contains("- none (running lanes are shown above)"));
	assert!(rendered.contains("run_id: run-1"));
	assert_eq!(rendered.matches("run_id: run-1").count(), 1);
	assert!(rendered.contains("attempt_status: running"));
	assert!(rendered.contains("phase: executing"));
	assert!(rendered.contains("current_operation: agent_run"));
	assert!(rendered.contains("queue_lease_state: held"));
	assert!(rendered.contains("queue_lease: held"));
	assert!(rendered.contains("execution_liveness: process_alive"));
	assert!(rendered.contains(
		"timing: run_idle=1 protocol_idle=1 last_progress=2026-03-14 10:00:01Z protocol_event=turn/completed @ 2026-03-14 10:00:01 events=4"
	));
	assert!(rendered.contains(
		"account: account=...acct01; plan=pro; status=selected; token=ok; primary=5h remaining=72%"
	));
	assert!(rendered.contains(
		"accounts: account=...acct01; plan=pro; status=selected"
	));
	assert!(rendered.contains(
		"account=...acct02; plan=plus; status=available; token=ok; primary=5h remaining=41%"
	));
	assert!(rendered.contains(
		"child_agent_activity: current=Model 10m52s; wall=12m14s; buckets=Model 11m33s, Browser/Image 41s; tool_calls=3"
	));
	assert!(rendered.contains(
		"protocol_activity: turn=completed; waiting=model_execution; rate_limit=none; recent=turn/completed:completed, item/tool/call:view_image"
	));
	assert!(rendered.contains(
		"context_pressure: input=current_window 105.0k, peak_window 105.0k (same as current), cumulative_input 4.27M; output_tokens=12.0k; largest_output=175.8KiB by view_image; warnings=view_image repeated 3 large outputs; largest 180000 bytes"
	));
	assert!(rendered.contains(
		"control_capability: status=active; transport=local_file; channel=.worktrees/PUB-101/.decodex-run-control/run-1-1.channel; thread_id=thread-1; turn_id=turn-1"
	));
	assert!(rendered.contains("turn_id: turn-1"));
	assert!(rendered.contains("thread_status: active"));
	assert!(rendered.contains("thread_active_flags: waitingOnApproval"));
	assert!(rendered.contains("interactive_requested: yes"));
	assert!(rendered.contains("effective_model: gpt-5.4"));
	assert!(rendered.contains("freshness_at: 2026-03-14 10:00:00Z"));
	assert!(rendered.contains("freshness_source: last_run_activity_at"));
	assert!(rendered.contains("updated_at: 2026-03-14 09:00:00"));
	assert!(rendered.contains("last_run_activity_at: 2026-03-14 10:00:00Z"));
	assert!(rendered.contains("last_progress_at: 2026-03-14 10:00:01Z"));
	assert!(rendered.contains("protocol_event: turn/completed @ 2026-03-14 10:00:01"));
	assert!(rendered.contains("Backlog: 1"));
	assert!(rendered.contains("Active queue echoes: 1"));
	assert!(rendered.contains("Stale closed queue labels: 1"));
	assert!(rendered.contains("Backlog"));
	assert!(rendered.contains("issue: PUB-102"));
	assert!(rendered.contains("classification: ready"));
	assert!(rendered.contains("Active Queue Echoes"));
	assert!(rendered.contains("issue: PUB-101"));
	assert!(rendered.contains("running_owner_run: run-1"));
	assert!(rendered.contains("Stale Closed Queue Labels"));
	assert!(rendered.contains("issue: PUB-105"));
	assert!(rendered.contains("classification: closed"));
	assert!(rendered.contains("Recovery worktrees: 2"));
	assert!(rendered.contains("Recovery Worktrees"));
	assert!(!rendered.contains("role: running_lane"));
	assert!(rendered.contains("role: post_review_lane"));
	assert!(rendered.contains("role: cleanup_only"));
	assert!(rendered.contains("worktree_path: .worktrees/PUB-103"));
	assert!(rendered.contains("worktree_path: .worktrees/PUB-104"));

	assert_recovery_worktree_roles_are_grouped(&rendered);
}

#[test]
fn operator_status_text_surfaces_execution_program_summary() {
	let snapshot = OperatorStatusSnapshot {
		project_id: String::from("decodex"),
		run_limit: 10,
		status_source: None,
		snapshot_age_seconds: None,
		warnings: Vec::new(),
		warning_details: Vec::new(),
		connector_backoffs: Vec::new(),
		projects: Vec::new(),
		account_control: OperatorCodexAccountControlStatus {
			mode: String::from("balanced"),
			account_selector: None,
		},
		accounts: Vec::new(),
		active_runs: Vec::new(),
		queued_candidates: Vec::new(),
		recent_runs: Vec::new(),
		history_lanes: Vec::new(),
		execution_programs: vec![OperatorExecutionProgramStatus {
			program_id: String::from("program-853"),
			source_contract_id: String::from("contract-852"),
			node_count: 3,
			planned_count: 0,
			mapped_count: 0,
			ready_count: 1,
			queued_count: 0,
			blocked_count: 1,
			held_count: 0,
			active_count: 0,
			needs_attention_count: 0,
			completed_count: 1,
			stale_count: 0,
			superseded_count: 0,
			queue_label_eligible_count: 1,
			mapped_issue_identifiers: vec![String::from("XY-853")],
			readback_warning: None,
		}],
		worktrees: Vec::new(),
		post_review_lanes: Vec::new(),
	};
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains("Execution programs: 1"));
	assert!(rendered.contains("Execution Programs"));
	assert!(rendered.contains(
		"program_id: program-853 source_contract_id: contract-852 nodes=3 planned=0 mapped=0 ready=1 queued=0 blocked=1 held=0 active=0 attention=0 completed=1 stale=0 superseded=0 queue_label_eligible=1 mapped_issues=XY-853"
	));
}

#[test]
fn operator_status_json_and_text_surface_loop_review_and_recovery_state() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	seed_loop_status_runs(&state_store, &config);
	seed_loop_status_review_checkpoints(&state_store, &config);
	seed_loop_status_private_events(&state_store, &config);

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
	let pending = active_run_json(&snapshot_json, "run-pending");
	let clean = active_run_json(&snapshot_json, "run-clean");
	let findings = active_run_json(&snapshot_json, "run-findings");
	let blocked = active_run_json(&snapshot_json, "run-blocked");

	assert_eq!(pending["loop_status"]["review_level"], "strict");
	assert_eq!(pending["loop_status"]["review"]["status"], "pending");
	assert_eq!(clean["loop_status"]["review"]["status"], "clean");
	assert_eq!(
		clean["loop_status"]["review"]["checkpoint"]["head_sha"],
		"1111111111111111111111111111111111111111"
	);
	assert_eq!(findings["loop_status"]["review"]["status"], "findings");
	assert_eq!(findings["loop_status"]["review"]["checkpoint"]["round"], 2);
	assert_eq!(
		findings["loop_status"]["architecture_recovery"]["status"],
		"active"
	);
	assert_eq!(findings["loop_status"]["autonomy"], "autonomous");
	assert_eq!(blocked["loop_status"]["review"]["status"], "blocked");
	assert_eq!(
		blocked["loop_status"]["architecture_recovery"]["status"],
		"exhausted"
	);
	assert_eq!(
		blocked["loop_status"]["boundary"]["disposition"],
		"requires_human"
	);
	assert_eq!(blocked["loop_status"]["autonomy"], "human_required");
	assert_eq!(
		blocked["loop_status"]["decision_request"]["decision_request_id"],
		"dr-pub-874-1"
	);

	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains("loop_review: phase=handoff status=pending checkpoint=none"));
	assert!(rendered.contains("loop_review: phase=handoff status=findings checkpoint=head:2222222222222222222222222222222222222222 round:2"));
	assert!(rendered.contains(
		"loop_architecture_recovery: status=active reason=architecture_recovery_started"
	));
	assert!(rendered.contains(
		"loop_status: human-required boundary stop: contract_boundary_required on accepted_behavior; review_level=strict; autonomy=human_required"
	));
	assert!(rendered.contains(
		"loop_boundary: disposition=requires_human reason=accepted behavior would change attempted_recovery=review_churn"
	));
}

fn seed_loop_status_runs(state_store: &StateStore, config: &ServiceConfig) {
	for (issue_id, run_id) in [
		("issue-pending", "run-pending"),
		("issue-clean", "run-clean"),
		("issue-findings", "run-findings"),
		("issue-blocked", "run-blocked"),
	] {
		state_store
			.record_run_attempt(run_id, issue_id, 1, "running")
			.expect("run attempt should record");
		state_store
			.upsert_lease(config.service_id(), issue_id, run_id, "In Progress")
			.expect("lease should record");
	}
}

fn seed_loop_status_review_checkpoints(state_store: &StateStore, config: &ServiceConfig) {
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: config.service_id(),
			issue_id: "issue-clean",
			run_id: "run-clean",
			attempt_number: 1,
			phase: "handoff",
			status: "clean",
			head_sha: "1111111111111111111111111111111111111111",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("clean checkpoint should record");
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: config.service_id(),
			issue_id: "issue-findings",
			run_id: "run-findings",
			attempt_number: 1,
			phase: "handoff",
			status: "findings",
			head_sha: "2222222222222222222222222222222222222222",
			nonclean_rounds: 2,
			details_json: "{}",
		})
		.expect("findings checkpoint should record");
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: config.service_id(),
			issue_id: "issue-blocked",
			run_id: "run-blocked",
			attempt_number: 1,
			phase: "repair",
			status: "blocked",
			head_sha: "3333333333333333333333333333333333333333",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("blocked checkpoint should record");
}

fn seed_loop_status_private_events(state_store: &StateStore, config: &ServiceConfig) {
	state_store
		.append_private_execution_event(
			config.service_id(),
			"issue-findings",
			"run-findings",
			1,
			"architecture_recovery_started",
			serde_json::json!({
				"reason_code": "architecture_recovery_started",
				"guardrail_reason": "validation_repeat",
				"recovery_budget": { "attempt": 1, "max_attempts": 2 },
			}),
		)
		.expect("active recovery should record");
	state_store
		.append_private_execution_event(
			config.service_id(),
			"issue-blocked",
			"run-blocked",
			1,
			"authority_boundary_check",
			serde_json::json!({
				"attempted_recovery_reason": "review_churn",
				"disposition": "requires_human",
				"final_disposition": {
					"disposition": "requires_human",
					"reason": "accepted behavior would change",
				},
				"changed_surfaces": [{ "surface": "runtime", "change_summary": "change behavior" }],
				"improvement_signals": [{ "kind": "missing_validator" }],
			}),
		)
		.expect("boundary check should record");
	state_store
		.append_private_execution_event(
			config.service_id(),
			"issue-blocked",
			"run-blocked",
			1,
			"architecture_recovery_terminal",
			serde_json::json!({
				"reason_code": "architecture_recovery_exhausted",
				"guardrail_reason": "review_churn",
				"boundary_disposition": "requires_human",
				"recovery_budget": { "attempt": 2, "max_attempts": 2 },
			}),
		)
		.expect("terminal recovery should record");
	state_store
		.append_private_execution_event(
			config.service_id(),
			"issue-blocked",
			"run-blocked",
			1,
			"authority_decision_request",
			serde_json::json!({
				"decision_request_id": "dr-pub-874-1",
				"phase": "human_required",
				"reason": "contract_boundary_required",
				"boundary": "accepted_behavior",
				"next_action": "accept or reject the recovery direction",
			}),
		)
		.expect("decision request should record");
}

fn active_run_json<'a>(snapshot_json: &'a Value, run_id: &str) -> &'a Value {
	snapshot_json["active_runs"]
		.as_array()
		.expect("active runs should be an array")
		.iter()
		.find(|run| run["run_id"] == run_id)
		.unwrap_or_else(|| panic!("active run `{run_id}` should exist"))
}

#[test]
fn queue_explain_renders_candidate_reasons_without_running_dispatch() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let candidates = operator_status_text_queued_candidates();
	let rendered = orchestrator::render_queue_explain(&config, &candidates);

	assert!(rendered.contains("Mode: dry-run queue explain"));
	assert!(rendered.contains("Queued candidates: 3"));
	assert!(rendered.contains("Ready: 1"));
	assert!(rendered.contains("Claimed: 1"));
	assert!(rendered.contains("Closed: 1"));
	assert!(rendered.contains("issue: PUB-102"));
	assert!(rendered.contains("classification: ready"));
	assert!(rendered.contains("reason: eligible_for_dispatch"));
}

#[test]
fn runtime_recovery_warning_keeps_safe_error_class() {
	assert_eq!(
		orchestrator::runtime_recovery_warning(
			"runtime_recovery_unavailable",
			&eyre::eyre!("Linear tracker request failed"),
		),
		"runtime_recovery_unavailable:tracker"
	);
	assert_eq!(
		orchestrator::runtime_recovery_warning(
			"runtime_recovery_unavailable",
			&eyre::eyre!("worktree scan failed"),
		),
		"runtime_recovery_unavailable:worktree"
	);
	assert_eq!(
		orchestrator::runtime_recovery_warning(
			"runtime_recovery_unavailable",
			&eyre::eyre!("sqlite runtime store locked"),
		),
		"runtime_recovery_unavailable:runtime_store"
	);
}

#[test]
fn operator_status_text_explains_empty_backlog_checks() {
	let snapshot = OperatorStatusSnapshot {
		project_id: String::from("pubfi"),
		run_limit: 10,
		status_source: None,
		snapshot_age_seconds: None,
		warnings: Vec::new(),
		warning_details: Vec::new(),
		connector_backoffs: Vec::new(),
		projects: Vec::new(),
		account_control: OperatorCodexAccountControlStatus {
			mode: String::from("balanced"),
			account_selector: None,
		},
		accounts: Vec::new(),
		active_runs: Vec::new(),
		queued_candidates: Vec::new(),
		recent_runs: Vec::new(),
		history_lanes: Vec::new(),
		execution_programs: Vec::new(),
		worktrees: Vec::new(),
		post_review_lanes: Vec::new(),
	};
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains("Backlog: 0"));
	assert!(rendered.contains("Hint: check `Todo`"));
	assert!(rendered.contains("`decodex:queued:<service-id>`"));
	assert!(rendered.contains("`decodex:queued:pubfi`"));
	assert!(rendered.contains("opt-out/manual-only"));
	assert!(rendered.contains("needs-attention"));
	assert!(rendered.contains("non-terminal state"));
	assert!(rendered.contains("dependency blockers"));
	assert!(rendered.contains("available capacity"));
}

#[test]
fn operator_status_text_surfaces_cleanup_blocker_pr_url() {
	let pr_url = "https://github.com/hack-ink/decodex/pull/119";
	let snapshot = OperatorStatusSnapshot {
		project_id: String::from("pubfi"),
		run_limit: 10,
		status_source: None,
		snapshot_age_seconds: None,
		warnings: Vec::new(),
		warning_details: Vec::new(),
		connector_backoffs: Vec::new(),
		projects: Vec::new(),
		account_control: OperatorCodexAccountControlStatus {
			mode: String::from("balanced"),
			account_selector: None,
		},
		accounts: Vec::new(),
		active_runs: Vec::new(),
		queued_candidates: Vec::new(),
		recent_runs: Vec::new(),
		history_lanes: Vec::new(),
		execution_programs: Vec::new(),
		worktrees: vec![orchestrator::OperatorWorktreeStatus {
			project_id: String::from("pubfi"),
			issue_id: String::from("issue-3"),
			issue_identifier: Some(String::from("PUB-103")),
			issue_state: Some(String::from("Done")),
			branch_name: String::from("x/pubfi-pub-103"),
			worktree_path: String::from(".worktrees/PUB-103"),
			ownership: String::from("post_review_lane"),
			ownership_reason: String::from(
				"Review & Landing owns this worktree as `cleanup_blocked`.",
			),
			provenance: orchestrator::OperatorWorktreeProvenanceStatus {
				source: String::from("runtime_recorded"),
				created_at_unix: Some(1),
				updated_at_unix: Some(2),
				audit_required: false,
			},
			recovery_next_action: None,
			hygiene: None,
		}],
		post_review_lanes: vec![orchestrator::OperatorPostReviewLaneStatus {
			project_id: String::from("pubfi"),
			issue_id: String::from("issue-3"),
			issue_identifier: String::from("PUB-103"),
			issue_state: String::from("Done"),
			branch_name: String::from("x/pubfi-pub-103"),
			worktree_path: String::from(".worktrees/PUB-103"),
			classification: String::from("cleanup_blocked"),
			reason: String::from("retry_budget_exhausted"),
			pr_url: Some(String::from(pr_url)),
			pr_head_sha: Some(String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6")),
			pr_state: Some(String::from("MERGED")),
			review_decision: Some(String::from("APPROVED")),
			mergeable: Some(String::from("MERGEABLE")),
			check_state: Some(String::from("SUCCESS")),
			unresolved_review_threads: Some(0),
			readback_warning: None,
			readback_root_cause: Some(String::from("lineage_validation_failed")),
			loop_status: None,
		}],
	};
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains("classification: cleanup_blocked"));
	assert!(rendered.contains("reason: retry_budget_exhausted"));
	assert!(rendered.contains("readback_root_cause: lineage_validation_failed"));
	assert!(rendered.contains(&format!("pr_url: {pr_url}")));
	assert!(!rendered.contains("pr_url: none"));
}

#[test]
fn operator_status_text_terminal_run_freshness_uses_terminal_update() {
	let mut terminal_run = operator_status_text_active_run();

	terminal_run.status = String::from("succeeded");
	terminal_run.phase = String::from("completed");
	terminal_run.active_lease = true;
	terminal_run.updated_at = String::from("2026-03-14 10:05:00");
	terminal_run.last_run_activity_at = Some(String::from("2026-03-14 10:10:00Z"));

	let history_lanes = orchestrator::operator_history_lanes(&[], &[terminal_run.clone()]);
	let snapshot = OperatorStatusSnapshot {
		project_id: String::from("pubfi"),
		run_limit: 10,
		status_source: None,
		snapshot_age_seconds: None,
		warnings: Vec::new(),
		warning_details: Vec::new(),
		connector_backoffs: Vec::new(),
		projects: Vec::new(),
		account_control: OperatorCodexAccountControlStatus {
			mode: String::from("balanced"),
			account_selector: None,
		},
		accounts: Vec::new(),
		active_runs: Vec::new(),
		queued_candidates: Vec::new(),
		recent_runs: vec![terminal_run],
		history_lanes,
		execution_programs: Vec::new(),
		worktrees: Vec::new(),
		post_review_lanes: Vec::new(),
	};
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains("run_id: run-1"));
	assert!(rendered.contains("phase: completed"));
	assert!(rendered.contains("active_lease: yes"));
	assert!(rendered.contains("freshness_at: 2026-03-14 10:05:00"));
	assert!(rendered.contains("freshness_source: updated_at"));
	assert!(rendered.contains("last_run_activity_at: 2026-03-14 10:10:00Z"));
}

#[test]
fn operator_status_text_active_run_without_live_activity_does_not_promote_updated_at() {
	let mut active_run = operator_status_text_active_run();

	active_run.updated_at = String::from("2026-03-14 09:00:00");
	active_run.last_run_activity_at = None;
	active_run.last_protocol_activity_at = None;
	active_run.last_progress_at = None;

	let snapshot = OperatorStatusSnapshot {
		project_id: String::from("pubfi"),
		run_limit: 10,
		status_source: None,
		snapshot_age_seconds: None,
		warnings: Vec::new(),
		warning_details: Vec::new(),
		connector_backoffs: Vec::new(),
		projects: Vec::new(),
		account_control: OperatorCodexAccountControlStatus {
			mode: String::from("balanced"),
			account_selector: None,
		},
		accounts: Vec::new(),
		active_runs: vec![active_run.clone()],
		queued_candidates: Vec::new(),
		recent_runs: vec![active_run],
		history_lanes: Vec::new(),
		execution_programs: Vec::new(),
		worktrees: Vec::new(),
		post_review_lanes: Vec::new(),
	};
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains("freshness_at: none"));
	assert!(rendered.contains("freshness_source: none"));
	assert!(rendered.contains("updated_at: 2026-03-14 09:00:00"));
}

#[test]
fn operator_status_text_explains_unleased_live_running_lane() {
	let mut active_run = operator_status_text_active_run();

	active_run.active_lease = false;
	active_run.queue_lease_state = String::from("not_held");

	let snapshot = OperatorStatusSnapshot {
		project_id: String::from("pubfi"),
		run_limit: 10,
		status_source: None,
		snapshot_age_seconds: None,
		warnings: Vec::new(),
		warning_details: Vec::new(),
		connector_backoffs: Vec::new(),
		projects: Vec::new(),
		account_control: OperatorCodexAccountControlStatus {
			mode: String::from("balanced"),
			account_selector: None,
		},
		accounts: Vec::new(),
		active_runs: vec![active_run.clone()],
		queued_candidates: Vec::new(),
		recent_runs: vec![active_run],
		history_lanes: Vec::new(),
		execution_programs: Vec::new(),
		worktrees: Vec::new(),
		post_review_lanes: Vec::new(),
	};
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains("active_lease: no"));
	assert!(rendered.contains("queue_lease_state: not_held"));
	assert!(rendered.contains("queue_lease: not_held (process_alive keeps lane visible)"));
	assert!(rendered.contains("execution_liveness: process_alive"));
}
