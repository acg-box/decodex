use crate::orchestrator::tests::operator::status::{
	self, OperatorCodexAccountControlStatus, OperatorGitHubCliAuthority, OperatorProjectStatus,
	OperatorStatusSnapshot, ProtocolActivitySummary, orchestrator, state,
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
