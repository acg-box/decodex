mod operator_status_output_tests {
	use std::io::{Error, ErrorKind, Result, Write};

	struct BrokenPipeWriter;

	struct FlushBrokenPipeWriter;

	impl Write for BrokenPipeWriter {
		fn write(&mut self, _buffer: &[u8]) -> Result<usize> {
			Err(Error::from(ErrorKind::BrokenPipe))
		}

		fn flush(&mut self) -> Result<()> {
			Ok(())
		}
	}

	impl Write for FlushBrokenPipeWriter {
		fn write(&mut self, buffer: &[u8]) -> Result<usize> {
			Ok(buffer.len())
		}

		fn flush(&mut self) -> Result<()> {
			Err(Error::from(ErrorKind::BrokenPipe))
		}
	}

	#[test]
	fn operator_status_output_accepts_closed_downstream_pipe() {
		let mut writer = BrokenPipeWriter;

		crate::orchestrator::write_cli_output(&mut writer, "partial status output\n")
			.expect("broken stdout pipe should be accepted");

		let mut writer = FlushBrokenPipeWriter;

		crate::orchestrator::write_cli_output(&mut writer, "buffered status output\n")
			.expect("broken stdout flush should be accepted");
	}
}

use crate::{
	execution_program::{
		ExecutionConflictDomain, ExecutionConflictDomainKind, ExecutionLinearIssueMapping,
		ExecutionProgram, ExecutionProgramDependency, ExecutionProgramNode,
		ExecutionProgramNodeStage, ExecutionQueueIntent,
	},
	loop_contract::{DecisionPromotion, DecisionPromotionActorKind},
	orchestrator::tests::operator::status::{
		self, Connection, DecisionContract, FakeTracker, HashMap,
		OperatorCodexAccountControlStatus, OperatorExecutionProgramNodeStatus,
		OperatorExecutionProgramStatus, OperatorGitHubCliAuthority, OperatorProjectStatus,
		OperatorStatusSnapshot, ProtocolActivitySummary, ReviewHandoffMarker,
		ReviewPolicyCheckpointInput, ServiceConfig, StateStore, Value, WorkflowDocument, env, eyre,
		orchestrator, state,
	},
};

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
		}],
		account_control: OperatorCodexAccountControlStatus {
			mode: String::from("balanced"),
			account_selector: None,
		},
		accounts: Vec::new(),
		current_lanes: Vec::new(),
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
	let current_lane = status::operator_status_text_current_lane();
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
		current_lanes: vec![current_lane.clone()],
		queued_candidates: status::operator_status_text_queued_candidates(),
		recent_runs: vec![current_lane],
		history_lanes: Vec::new(),
		execution_programs: Vec::new(),
		worktrees: status::operator_status_text_worktrees(),
		post_review_lanes: status::operator_status_text_post_review_lanes(),
	};
	let rendered = orchestrator::render_operator_status(&snapshot);
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
	let current_lane_json = &snapshot_json["current_lanes"][0];

	assert!(rendered.contains("Project: pubfi"));
	assert!(rendered.contains("Warnings: 0"));
	assert!(rendered.contains("Current Lanes"));
	assert!(rendered.contains(
		"Run ledger shown: 0 issue lanes from 0 history attempts (current lanes inline)"
	));
	assert!(rendered.contains("Run Ledger"));
	assert!(rendered.contains("- none (current lanes are shown above)"));
	assert!(rendered.contains("run_id: run-1"));
	assert_eq!(rendered.matches("run_id: run-1").count(), 1);
	assert!(rendered.contains("attempt_status: running"));
	assert!(rendered.contains("run_phase: executing"));
	assert!(rendered.contains("current_operation: agent_run"));
	assert!(rendered.contains("active_goal_phase: implement_to_validation_ready"));
	assert!(rendered.contains("public_progress_phase: implementing"));
	assert_eq!(current_lane_json["run_phase"], "executing");
	assert_eq!(current_lane_json["active_goal_phase"], "implement_to_validation_ready");
	assert_eq!(current_lane_json["public_progress_phase"], "implementing");
	assert!(rendered.contains("queue_lease_state: held"));
	assert!(rendered.contains("queue_lease: held"));
	assert!(rendered.contains("execution_liveness: process_alive"));
	assert!(rendered.contains("has_fresh_execution: yes"));
	assert!(rendered.contains("counts_as_running: yes"));
	assert!(rendered.contains("needs_attention: no"));
	assert!(rendered.contains(
		"timing: run_idle=1 protocol_idle=1 last_progress=2026-03-14 10:00:01Z protocol_event=turn/completed @ 2026-03-14 10:00:01 events=4"
	));
	assert!(rendered.contains(
		"account: account=...acct01; plan=pro; status=selected; token=ok; primary=5h remaining=72%"
	));
	assert!(rendered.contains("accounts: account=...acct01; plan=pro; status=selected"));
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
	assert!(rendered.contains("Claimed queue echoes: 1"));
	assert!(rendered.contains("Stale closed queue labels: 1"));
	assert!(rendered.contains("Backlog"));
	assert!(rendered.contains("issue: PUB-102"));
	assert!(rendered.contains("classification: ready"));
	assert!(rendered.contains("Claimed Queue Echoes"));
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

	status::assert_recovery_worktree_roles_are_grouped(&rendered);
}

#[test]
fn operator_status_text_sanitizes_private_protocol_activity_details() {
	let mut current_lane = status::operator_status_text_current_lane();

	current_lane.protocol_activity = Some(ProtocolActivitySummary {
		turn_status: Some(String::from("completed")),
		waiting_reason: Some(String::from("turn_completed")),
		rate_limit_status: None,
		recent_events: vec![
			state::ProtocolActivityEventSummary {
				event_type: String::from("error"),
				category: String::from("protocol_error"),
				detail: Some(String::from(
					"upstream auth failed for ghp_abcdefghijklmnopqrstuvwxyz123456",
				)),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("configWarning"),
				category: String::from("warning"),
				detail: Some(String::from("config at /private/worktree using GITHUB_PAT_Y")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("configWarning"),
				category: String::from("warning"),
				detail: Some(String::from("state marker under /srv/decodex/runtime")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("configWarning"),
				category: String::from("warning"),
				detail: Some(String::from("state marker path=/srv/decodex/runtime")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("configWarning"),
				category: String::from("warning"),
				detail: Some(String::from("state marker (/srv/decodex/runtime)")),
			},
		],
	});

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
		current_lanes: vec![current_lane],
		queued_candidates: Vec::new(),
		recent_runs: Vec::new(),
		history_lanes: Vec::new(),
		execution_programs: Vec::new(),
		worktrees: Vec::new(),
		post_review_lanes: Vec::new(),
	};
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains(
		"protocol_activity: turn=completed; waiting=turn_completed; rate_limit=none; recent=configWarning:redacted_sensitive_detail, configWarning:redacted_sensitive_detail, configWarning:redacted_sensitive_detail, configWarning:redacted_sensitive_detail, error:redacted_sensitive_detail"
	));
	assert!(!rendered.contains("GITHUB_PAT_Y"));
	assert!(!rendered.contains("ghp_"));
	assert!(!rendered.contains("/private/worktree"));
	assert!(!rendered.contains("/srv/decodex/runtime"));
	assert!(!rendered.contains("path=/srv"));
	assert!(!rendered.contains("(/srv"));
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
		current_lanes: Vec::new(),
		queued_candidates: Vec::new(),
		recent_runs: Vec::new(),
		history_lanes: Vec::new(),
		execution_programs: vec![OperatorExecutionProgramStatus {
			program_id: String::from("program-853"),
			status: String::from("blocked"),
			source_contract_id: Some(String::from("contract-852")),
			intake_kind: Some(String::from("goal_intake")),
			public_summary: Some(String::from("Resolve promoted program work.")),
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
			dispatchable_count: 0,
			mapped_issue_identifiers: vec![String::from("XY-853")],
			node_readbacks: vec![OperatorExecutionProgramNodeStatus {
				program_stage: String::from("runtime"),
				lifecycle_state: String::from("blocked"),
				readiness_state: String::from("blocked"),
				issue_identifier: Some(String::from("XY-853")),
				issue_state: Some(String::from("Todo")),
				dispatch_action: None,
				reason_codes: vec![String::from("dependency_not_terminal")],
				reasons: vec![String::from(
					"a dependency has not reached a required terminal state",
				)],
				next_action: String::from(
					"Complete the dependency issue or refresh the Execution Program dependency plan if this remains stale.",
				),
			}],
			readback_warning: None,
		}],
		worktrees: Vec::new(),
		post_review_lanes: Vec::new(),
	};
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains("Execution programs: 1"));
	assert!(rendered.contains("Execution Programs"));
	assert!(rendered.contains(
		"program_id: program-853 status=blocked source_contract_id: contract-852 intake_kind=goal_intake summary=\"Resolve promoted program work.\" nodes=3 planned=0 mapped=0 ready=1 queued=0 blocked=1 held=0 active=0 attention=0 completed=1 stale=0 superseded=0 dispatchable=0 mapped_issues=XY-853"
	));
	assert!(rendered.contains(
		"node: issue=XY-853 issue_state=Todo program_stage=runtime lifecycle=blocked readiness=blocked dispatch_action=none reason_codes=dependency_not_terminal reasons=\"a dependency has not reached a required terminal state\" next_action=\"Complete the dependency issue or refresh the Execution Program dependency plan if this remains stale.\""
	));
}

#[test]
fn operator_status_json_uses_direct_dispatch_program_fields() {
	let snapshot: OperatorStatusSnapshot = serde_json::from_value(serde_json::json!({
		"project_id": "decodex",
		"run_limit": 10,
		"warnings": [],
		"warning_details": [],
		"connector_backoffs": [],
		"projects": [],
		"account_control": {
			"mode": "balanced",
			"account_selector": null,
		},
		"accounts": [],
		"current_lanes": [],
		"recent_runs": [],
		"history_lanes": [],
		"execution_programs": [{
			"program_id": "direct-dispatch-program",
			"source_contract_id": null,
			"node_count": 1,
			"planned_count": 0,
			"mapped_count": 0,
			"ready_count": 1,
			"queued_count": 0,
			"blocked_count": 0,
			"held_count": 0,
			"active_count": 0,
			"needs_attention_count": 0,
			"completed_count": 0,
			"stale_count": 0,
			"superseded_count": 0,
			"dispatchable_count": 1,
			"mapped_issue_identifiers": ["XY-853"],
			"node_readbacks": [{
				"lifecycle_state": "ready",
				"readiness_state": "ready",
				"issue_identifier": "XY-853",
				"issue_state": "Todo",
				"dispatch_action": "dispatch",
				"reason_codes": ["ready_for_linear_execution"],
				"reasons": ["node is ready for normal Linear issue execution"],
				"next_action": "The program scheduler can dispatch this node directly."
			}],
			"readback_warning": null,
		}],
		"queued_candidates": [],
		"worktrees": [],
		"post_review_lanes": [],
	}))
	.expect("operator snapshot should deserialize");
	let program = snapshot.execution_programs.first().expect("program should deserialize");

	assert_eq!(program.status, "unknown");
	assert_eq!(program.intake_kind, None);
	assert_eq!(program.public_summary, None);
	assert_eq!(program.dispatchable_count, 1);
	assert_eq!(program.node_readbacks[0].dispatch_action.as_deref(), Some("dispatch"));
}

#[test]
fn operator_status_snapshot_surfaces_program_intake_and_node_readbacks() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	seed_program_readback_status(&state_store, &config);

	let snapshot = build_program_readback_snapshot(&config, &workflow, &state_store);
	let program = snapshot.execution_programs.first().expect("program should surface");
	let program_json = program_readback_json(&snapshot);

	assert_program_readback_summary(program);
	assert_program_readback_json(&program_json);
	assert_program_node_readbacks(program, &program_json);
}

#[test]
fn operator_status_program_readback_prefers_post_review_owner_over_stale_active_label() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_id = "issue-post-review";
	let issue_identifier = "PUB-946";
	let branch_name = "x/pubfi-pub-946";
	let conflict = ExecutionConflictDomain::new(ExecutionConflictDomainKind::Module, "runtime")
		.expect("conflict domain should build");
	let node =
		status_program_active_node("node-post-review", issue_id, issue_identifier, "In Review")
			.with_conflict_domains([conflict])
			.expect("conflict domain should attach");
	let program = ExecutionProgram::from_issue_batch_intake(
		"program-post-review-owner",
		config.service_id(),
		"program-post-review-owner-fingerprint",
		"Track post-review owner readback.",
		vec![node],
	)
	.expect("program should build");

	state_store
		.upsert_execution_program(config.service_id(), program)
		.expect("program should persist");
	state_store
		.upsert_review_handoff_marker(
			config.service_id(),
			issue_id,
			&ReviewHandoffMarker::new(
				"pub-946-attempt-1",
				1,
				branch_name,
				"https://github.com/hack-ink/pubfi/pull/946",
				"main",
				branch_name,
				"1111111111111111111111111111111111111111",
			),
		)
		.expect("review lifecycle should persist");

	let snapshot = build_program_readback_snapshot(&config, &workflow, &state_store);
	let program = snapshot.execution_programs.first().expect("program should surface");
	let node = program.node_readbacks.first().expect("post-review node should surface");

	assert_eq!(program.status, "active");
	assert_eq!(program.active_count, 1);
	assert_eq!(program.blocked_count, 0);
	assert_eq!(node.lifecycle_state, "post_review");
	assert_eq!(node.readiness_state, "blocked");
	assert!(node.reason_codes.contains(&String::from("mapped_issue_post_review_owner")));
	assert!(!node.reason_codes.contains(&String::from("mapped_issue_active_label_present")));
	assert!(!node.reason_codes.contains(&String::from("conflict_domain_occupied")));
	assert_eq!(
		node.reasons,
		vec![String::from(
			"Review & Landing owns this issue until post-review landing or closeout finishes",
		)]
	);
	assert_eq!(
		node.next_action,
		"Continue the retained post-review lifecycle before dispatching this program node."
	);
}

#[test]
fn operator_status_program_readback_refreshes_live_tracker_issue_mapping() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let stale_mapping = status_program_issue_mapping("issue-live-refresh", "PUB-1597", "Todo")
		.with_needs_attention_label(true);
	let node = ExecutionProgramNode::new(
		"node-live-refresh",
		ExecutionProgramNodeStage::Runtime,
		"Close stale Program attention after the mapped issue is terminal.",
		ExecutionQueueIntent::ReadyToQueue,
	)
	.expect("node should build")
	.with_acceptance_expectations(["The mapped issue reflects live tracker state."])
	.expect("acceptance should attach")
	.with_validation_expectations(["Build operator status."])
	.expect("validation should attach")
	.with_linear_issue(stale_mapping)
	.expect("stale mapping should attach");
	let program = ExecutionProgram::from_issue_batch_intake(
		"program-live-refresh",
		config.service_id(),
		"program-live-refresh-fingerprint",
		"Refresh live Program issue metadata.",
		vec![node],
	)
	.expect("program should build");

	state_store
		.upsert_execution_program(config.service_id(), program)
		.expect("program should persist");

	let live_issue = status::sample_issue_with_sort_fields(
		"issue-live-refresh",
		"PUB-1597",
		"Done",
		&[],
		Some(1),
		"2026-06-19T00:00:00.000Z",
	);
	let tracker = FakeTracker::new(vec![live_issue]);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("status snapshot should build");
	let program = snapshot.execution_programs.first().expect("program should surface");

	assert_eq!(program.status, "completed");
	assert_eq!(program.completed_count, 1);
	assert_eq!(program.needs_attention_count, 0);
	assert_eq!(program.blocked_count, 0);
	assert_eq!(program.dispatchable_count, 0);
	assert!(
		program.node_readbacks.is_empty(),
		"terminal refreshed Program nodes should not render stale attention readbacks"
	);
	assert_eq!(tracker.refresh_queries.borrow().len(), 1);
	assert_eq!(tracker.refresh_queries.borrow()[0], vec![String::from("issue-live-refresh")]);
}

fn seed_program_readback_status(state_store: &StateStore, config: &ServiceConfig) {
	let program = ExecutionProgram::from_issue_batch_intake(
		"program-status-readback",
		config.service_id(),
		"program-status-fingerprint",
		"Coordinate status readback work.",
		vec![
			status_program_node(
				"node-ready",
				"issue-ready",
				"PUB-941",
				"Todo",
				ExecutionQueueIntent::ReadyToQueue,
			),
			status_program_node(
				"node-queued",
				"issue-queued",
				"PUB-942",
				"Todo",
				ExecutionQueueIntent::Queued,
			),
			status_program_node_with_dependency(
				"node-blocked",
				"issue-blocked",
				"PUB-943",
				"Todo",
				"PUB-944",
			),
			status_program_node(
				"node-dependency",
				"issue-dependency",
				"PUB-944",
				"Todo",
				ExecutionQueueIntent::NotReady,
			),
			status_program_active_node("node-active", "issue-active", "PUB-945", "In Progress"),
		],
	)
	.expect("program should build");

	state_store
		.upsert_execution_program(config.service_id(), program)
		.expect("program should persist");
}

fn build_program_readback_snapshot(
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> OperatorStatusSnapshot {
	let tracker = FakeTracker::new(Vec::new());

	orchestrator::build_live_operator_status_snapshot(&tracker, config, workflow, state_store, 10)
		.expect("status snapshot should build")
}

fn assert_program_readback_summary(program: &OperatorExecutionProgramStatus) {
	assert_eq!(program.program_id, "program-status-readback");
	assert_eq!(program.status, "blocked");
	assert_eq!(program.intake_kind.as_deref(), Some("issue_batch_intake"));
	assert_eq!(program.public_summary.as_deref(), Some("Coordinate status readback work."));
	assert_eq!(program.ready_count, 2);
	assert_eq!(program.queued_count, 0);
	assert_eq!(program.blocked_count, 1);
	assert_eq!(program.held_count, 2);
	assert_eq!(program.active_count, 1);
	assert_eq!(program.stale_count, 0);
	assert_eq!(program.dispatchable_count, 2);
	assert_eq!(
		program.mapped_issue_identifiers,
		vec![
			String::from("PUB-941"),
			String::from("PUB-942"),
			String::from("PUB-943"),
			String::from("PUB-944"),
			String::from("PUB-945"),
		]
	);
}

fn program_readback_json(snapshot: &OperatorStatusSnapshot) -> Value {
	let snapshot_json = serde_json::to_value(snapshot).expect("snapshot should serialize");

	snapshot_json["execution_programs"]
		.as_array()
		.expect("execution programs should serialize as an array")
		.first()
		.expect("program should serialize")
		.clone()
}

fn assert_program_readback_json(program_json: &Value) {
	assert_eq!(program_json["program_id"], "program-status-readback");
	assert_eq!(program_json["status"], "blocked");
	assert_eq!(program_json["intake_kind"], "issue_batch_intake");
	assert_eq!(program_json["public_summary"], "Coordinate status readback work.");
	assert_eq!(program_json["ready_count"], 2);
	assert_eq!(program_json["queued_count"], 0);
	assert_eq!(program_json["active_count"], 1);
	assert_eq!(program_json["held_count"], 2);
	assert_eq!(program_json["dispatchable_count"], 2);
	assert!(program_json.get("contract").is_none());
	assert!(program_json.get("graph").is_none());
}

fn assert_program_node_readbacks(program: &OperatorExecutionProgramStatus, program_json: &Value) {
	let node_by_issue = program
		.node_readbacks
		.iter()
		.filter_map(|node| node.issue_identifier.as_deref().map(|issue| (issue, node)))
		.collect::<HashMap<_, _>>();
	let ready_node = node_by_issue.get("PUB-941").expect("ready node should render");
	let queued_node = node_by_issue.get("PUB-942").expect("queued node should render");
	let blocked_node = node_by_issue.get("PUB-943").expect("blocked node should render");
	let held_node = node_by_issue.get("PUB-944").expect("held node should render");
	let active_node = node_by_issue.get("PUB-945").expect("active node should render");

	assert_eq!(ready_node.dispatch_action.as_deref(), Some("dispatch"));
	assert_eq!(queued_node.dispatch_action.as_deref(), Some("dispatch"));
	assert_eq!(blocked_node.lifecycle_state, "blocked");
	assert_eq!(blocked_node.dispatch_action.as_deref(), None);
	assert!(blocked_node.reason_codes.contains(&String::from("dependency_not_terminal")));
	assert_eq!(
		blocked_node.reasons,
		vec![String::from("a dependency has not reached a required terminal state")]
	);
	assert!(blocked_node.next_action.contains("Execution Program dependency plan"));
	assert_eq!(held_node.lifecycle_state, "mapped");
	assert!(held_node.reason_codes.contains(&String::from("dispatch_intent_not_ready")));
	assert_eq!(active_node.lifecycle_state, "active");
	assert!(active_node.reason_codes.contains(&String::from("mapped_issue_active_label_present")));

	let node_json = program_json["node_readbacks"]
		.as_array()
		.expect("node readbacks should serialize as an array")
		.iter()
		.find(|node| node["issue_identifier"] == "PUB-945")
		.expect("active node json should serialize");

	assert_eq!(node_json["lifecycle_state"], "active");
	assert_eq!(node_json["readiness_state"], "blocked");
	assert_eq!(node_json["program_stage"], "runtime");
	assert_eq!(node_json["dispatch_action"], serde_json::Value::Null);
}

#[test]
fn operator_status_json_surfaces_missing_contract_program_recovery() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let contract = accepted_status_decision_contract_fixture();
	let program = ExecutionProgram::from_accepted_contract(
		"program-missing-contract",
		config.service_id(),
		&contract,
		vec![status_program_node(
			"node-stale",
			"issue-stale",
			"PUB-946",
			"Todo",
			ExecutionQueueIntent::ReadyToQueue,
		)],
	)
	.expect("program should build");

	state_store
		.upsert_execution_program(config.service_id(), program)
		.expect("program should persist");

	let tracker = FakeTracker::new(Vec::new());
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("status snapshot should build");
	let program = snapshot.execution_programs.first().expect("program should surface");

	assert_eq!(program.program_id, "program-missing-contract");
	assert_eq!(program.status, "stale");
	assert_eq!(program.source_contract_id.as_deref(), Some(contract.contract_id()));
	assert_eq!(program.intake_kind.as_deref(), Some("goal_intake"));
	assert_eq!(program.stale_count, 1);
	assert_eq!(program.readback_warning.as_deref(), Some("source_decision_contract_missing"));
	assert_eq!(program.mapped_issue_identifiers, vec![String::from("PUB-946")]);

	let node = program.node_readbacks.first().expect("stale node should render");

	assert_eq!(node.lifecycle_state, "stale");
	assert_eq!(node.readiness_state, "stale");
	assert_eq!(node.issue_identifier.as_deref(), Some("PUB-946"));
	assert_eq!(node.reason_codes, vec![String::from("source_decision_contract_missing")]);
	assert!(node.next_action.contains("Decision Contract"));

	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
	let program_json = snapshot_json["execution_programs"]
		.as_array()
		.expect("execution programs should serialize as an array")
		.first()
		.expect("program should serialize");

	assert_eq!(program_json["status"], "stale");
	assert_eq!(program_json["readback_warning"], "source_decision_contract_missing");
	assert_eq!(program_json["node_readbacks"][0]["program_stage"], "runtime");
	assert_eq!(
		program_json["node_readbacks"][0]["reason_codes"][0],
		"source_decision_contract_missing"
	);
	assert_eq!(
		program_json["node_readbacks"][0]["next_action"],
		"Restore or supersede the source Decision Contract before dispatching this program."
	);
	assert!(program_json.get("contract").is_none());
	assert!(program_json.get("decision_contract").is_none());
}

#[test]
fn operator_status_readback_uses_migrated_removed_flat_decision_contract_fields() {
	let (temp_dir, config, workflow) = status::temp_project_layout();
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let state_store = StateStore::open(&state_path).expect("state store should open");
	let contract = accepted_status_decision_contract_fixture();
	let program = ExecutionProgram::from_accepted_contract(
		"program-removed-flat-contract",
		config.service_id(),
		&contract,
		vec![status_program_node(
			"node-removed-flat",
			"issue-removed-flat",
			"PUB-947",
			"Todo",
			ExecutionQueueIntent::ReadyToQueue,
		)],
	)
	.expect("program should build");

	state_store
		.upsert_execution_program(config.service_id(), program)
		.expect("program should persist");

	let mut removed_field_payload =
		serde_json::to_value(&contract).expect("contract should encode as JSON");
	let readiness = removed_field_payload
		.get_mut("execution_readiness")
		.expect("readiness should exist")
		.as_object_mut()
		.expect("readiness should be an object");

	readiness.remove("proposed_issues");
	readiness.insert(
		String::from("proposed_issue_summaries"),
		serde_json::json!(["Flat summary that must be migrated before readback."]),
	);
	readiness.insert(
		String::from("queue_intent"),
		serde_json::json!(["Removed queue intent that must not be re-admitted."]),
	);

	{
		let connection = Connection::open(&state_path).expect("sqlite should open");

		connection
			.execute(
				"INSERT INTO decision_contracts (
						project_id, contract_id, source_issue_id, status, payload_json, created_at,
						created_at_unix, updated_at, updated_at_unix
					) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
				rusqlite::params![
					config.service_id(),
					contract.contract_id(),
					"PUB-947",
					contract.status().as_str(),
					serde_json::to_string(&removed_field_payload)
						.expect("removed-field payload should serialize"),
					"2026-06-17T00:00:00Z",
					1_i64,
					"2026-06-17T00:00:00Z",
					1_i64,
				],
			)
			.expect("removed-field decision contract row should insert");
		connection
			.execute("UPDATE schema_meta SET value = '11' WHERE key = 'schema_version'", [])
			.expect("schema version should mark removed-field state");
	}

	drop(state_store);

	let state_store = StateStore::open(&state_path).expect("removed fields should migrate");
	let migrated_contract = state_store
		.decision_contract(config.service_id(), contract.contract_id())
		.expect("migrated contract read should succeed")
		.expect("migrated contract should exist");

	assert_eq!(
		migrated_contract.contract().execution_readiness().proposed_issues()[0].objective(),
		"Flat summary that must be migrated before readback."
	);

	let snapshot = build_program_readback_snapshot(&config, &workflow, &state_store);
	let program = snapshot.execution_programs.first().expect("program should surface");

	assert_eq!(program.program_id, "program-removed-flat-contract");
	assert_eq!(program.status, "stale");
	assert_ne!(program.readback_warning.as_deref(), Some("source_decision_contract_missing"));
	assert_eq!(program.stale_count, 1);
	assert_eq!(program.mapped_issue_identifiers, vec![String::from("PUB-947")]);
}

fn status_program_node(
	node_id: &str,
	issue_id: &str,
	issue_identifier: &str,
	issue_state: &str,
	queue_intent: ExecutionQueueIntent,
) -> ExecutionProgramNode {
	let mapping = status_program_issue_mapping(issue_id, issue_identifier, issue_state);

	ExecutionProgramNode::new(
		node_id,
		ExecutionProgramNodeStage::Runtime,
		format!("Resolve {issue_identifier}."),
		queue_intent,
	)
	.expect("node should build")
	.with_acceptance_expectations([format!("{issue_identifier} acceptance is explicit.")])
	.expect("acceptance should attach")
	.with_validation_expectations([String::from("Run focused validation.")])
	.expect("validation should attach")
	.with_linear_issue(mapping)
	.expect("mapping should attach")
}

fn status_program_node_with_dependency(
	node_id: &str,
	issue_id: &str,
	issue_identifier: &str,
	issue_state: &str,
	dependency_identifier: &str,
) -> ExecutionProgramNode {
	status_program_node(
		node_id,
		issue_id,
		issue_identifier,
		issue_state,
		ExecutionQueueIntent::ReadyToQueue,
	)
	.with_dependencies([
		ExecutionProgramDependency::new(dependency_identifier).expect("dependency should build")
	])
	.expect("dependency should attach")
}

fn status_program_issue_mapping(
	issue_id: &str,
	issue_identifier: &str,
	issue_state: &str,
) -> ExecutionLinearIssueMapping {
	ExecutionLinearIssueMapping::new(issue_id, issue_identifier, issue_state)
		.expect("mapping should build")
}

fn status_program_active_node(
	node_id: &str,
	issue_id: &str,
	issue_identifier: &str,
	issue_state: &str,
) -> ExecutionProgramNode {
	let mapping = status_program_issue_mapping(issue_id, issue_identifier, issue_state)
		.with_active_label(true);

	ExecutionProgramNode::new(
		node_id,
		ExecutionProgramNodeStage::Runtime,
		format!("Resolve {issue_identifier}."),
		ExecutionQueueIntent::ReadyToQueue,
	)
	.expect("node should build")
	.with_acceptance_expectations([format!("{issue_identifier} acceptance is explicit.")])
	.expect("acceptance should attach")
	.with_validation_expectations([String::from("Run focused validation.")])
	.expect("validation should attach")
	.with_linear_issue(mapping)
	.expect("mapping should attach")
}

fn accepted_status_decision_contract_fixture() -> DecisionContract {
	let mut contract: DecisionContract = serde_json::from_str(include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/fixtures/decision_contract/research_x_latent_contract.json"
	)))
	.expect("decision contract fixture should deserialize");

	contract
		.promote(
			DecisionPromotion::new(
				"operator",
				DecisionPromotionActorKind::User,
				"2026-06-09T10:00:00Z",
				"conversation",
				Some(String::from("User accepted the program boundary.")),
			)
			.expect("promotion should build"),
		)
		.expect("contract should promote");

	contract
}

#[test]
fn operator_status_json_and_text_surface_loop_review_and_recovery_state() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	seed_loop_status_runs(&state_store, &config);
	seed_loop_status_review_checkpoints(&state_store, &config);
	seed_loop_status_private_events(&state_store, &config);

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
	let pending = status_run_json(&snapshot_json, "run-pending");
	let clean = status_run_json(&snapshot_json, "run-clean");
	let findings = status_run_json(&snapshot_json, "run-findings");
	let blocked = status_run_json(&snapshot_json, "run-blocked");

	assert_eq!(pending["loop_status"]["review_level"], "strict");
	assert_eq!(pending["loop_status"]["review"]["status"], "pending");
	assert_eq!(clean["loop_status"]["review"]["status"], "clean");
	assert_eq!(
		clean["loop_status"]["review"]["checkpoint"]["head_sha"],
		"1111111111111111111111111111111111111111"
	);
	assert_eq!(findings["loop_status"]["review"]["status"], "findings");
	assert_eq!(findings["loop_status"]["review"]["checkpoint"]["round"], 2);
	assert_eq!(findings["loop_status"]["architecture_recovery"]["status"], "active");
	assert_eq!(findings["loop_status"]["autonomy"], "autonomous");
	assert_eq!(blocked["loop_status"]["review"]["status"], "blocked");
	assert_eq!(blocked["loop_status"]["architecture_recovery"]["status"], "exhausted");
	assert_eq!(blocked["loop_status"]["boundary"]["disposition"], "requires_human");
	assert_eq!(blocked["loop_status"]["boundary"]["policy_decision"], "requires_human_decision");
	assert_eq!(blocked["loop_status"]["autonomy"], "human_required");
	assert_eq!(blocked["loop_status"]["decision_request"]["decision_request_id"], "dr-pub-874-1");

	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains(
		"loop_review: phase=handoff status=pending checkpoint=head:0000000000000000000000000000000000000000 round:0"
	));
	assert!(rendered.contains("loop_review: phase=handoff status=findings checkpoint=head:2222222222222222222222222222222222222222 round:2"));
	assert!(rendered.contains(
		"loop_architecture_recovery: status=active reason=architecture_recovery_started"
	));
	assert!(rendered.contains(
		"loop_status: human-required boundary stop: contract_boundary_required on accepted_behavior; review_level=strict; autonomy=human_required"
	));
	assert!(rendered.contains(
		"loop_boundary: disposition=requires_human policy=requires_human_decision enhanced_evidence=false blocks_landing=false reason=accepted behavior would change attempted_recovery=review_churn"
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
			issue_id: "issue-pending",
			run_id: "run-pending",
			attempt_number: 1,
			phase: "handoff",
			review_level: config.codex().review_level().as_str(),
			status: "pending",
			head_sha: "0000000000000000000000000000000000000000",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("pending checkpoint should record");
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: config.service_id(),
			issue_id: "issue-clean",
			run_id: "run-clean",
			attempt_number: 1,
			phase: "handoff",
			review_level: config.codex().review_level().as_str(),
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
			review_level: config.codex().review_level().as_str(),
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
			review_level: config.codex().review_level().as_str(),
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

fn status_run_json<'a>(snapshot_json: &'a Value, run_id: &str) -> &'a Value {
	for collection in ["current_lanes", "recent_runs"] {
		if let Some(run) = snapshot_json[collection]
			.as_array()
			.expect("status run collection should be an array")
			.iter()
			.find(|run| run["run_id"] == run_id)
		{
			return run;
		}
	}

	status::panic!("status run `{run_id}` should exist")
}

#[test]
fn queue_explain_renders_candidate_reasons_without_running_dispatch() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let candidates = status::operator_status_text_queued_candidates();
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
		current_lanes: Vec::new(),
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
	assert!(rendered.contains("no active issue claim"));
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
		current_lanes: Vec::new(),
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
			shadowed_by_current_lane: false,
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
	let mut terminal_run = status::operator_status_text_current_lane();

	terminal_run.status = String::from("succeeded");
	terminal_run.phase = String::from("completed");
	terminal_run.run_phase = String::from("completed");
	terminal_run.run_lease = true;
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
		current_lanes: Vec::new(),
		queued_candidates: Vec::new(),
		recent_runs: vec![terminal_run],
		history_lanes,
		execution_programs: Vec::new(),
		worktrees: Vec::new(),
		post_review_lanes: Vec::new(),
	};
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains("run_id: run-1"));
	assert!(rendered.contains("run_phase: completed"));
	assert!(rendered.contains("run_lease: yes"));
	assert!(rendered.contains("freshness_at: 2026-03-14 10:05:00"));
	assert!(rendered.contains("freshness_source: updated_at"));
	assert!(rendered.contains("last_run_activity_at: 2026-03-14 10:10:00Z"));
}

#[test]
fn operator_status_text_current_lane_without_live_activity_does_not_promote_updated_at() {
	let mut current_lane = status::operator_status_text_current_lane();

	current_lane.updated_at = String::from("2026-03-14 09:00:00");
	current_lane.last_run_activity_at = None;
	current_lane.last_protocol_activity_at = None;
	current_lane.last_progress_at = None;

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
		current_lanes: vec![current_lane.clone()],
		queued_candidates: Vec::new(),
		recent_runs: vec![current_lane],
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
	let mut current_lane = status::operator_status_text_current_lane();

	current_lane.run_lease = false;
	current_lane.queue_lease_state = String::from("not_held");
	current_lane.attempt_status = String::from("stalled");
	current_lane.status_projection_reason =
		Some(String::from("terminal_attempt_promoted_by_process_alive"));

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
		current_lanes: vec![current_lane.clone()],
		queued_candidates: Vec::new(),
		recent_runs: vec![current_lane],
		history_lanes: Vec::new(),
		execution_programs: Vec::new(),
		worktrees: Vec::new(),
		post_review_lanes: Vec::new(),
	};
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains("run_lease: no"));
	assert!(rendered.contains("queue_lease_state: not_held"));
	assert!(rendered.contains("queue_lease: not_held (process_alive keeps lane visible)"));
	assert!(
		rendered.contains("status_projection_reason: terminal_attempt_promoted_by_process_alive")
	);
	assert!(rendered.contains("execution_liveness: process_alive"));
}
