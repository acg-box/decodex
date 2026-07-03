use crate::{
	orchestrator::{
		AgentPrivateEvidenceRef, OperatorLaneLifecycleMetrics, OperatorPostReviewLaneStatus,
		OperatorQueuedIssueStatus, OperatorRunControlCapability, OperatorRunStatus,
		OperatorWorktreeProvenanceStatus, OperatorWorktreeStatus, tests::operator::TEST_SERVICE_ID,
	},
	state::{
		self, ChildAgentActivityBucket, ChildAgentActivitySummary, ProtocolActivityEventSummary,
		ProtocolActivitySummary, RUN_OPERATION_AGENT_RUN,
	},
};

pub(super) fn operator_status_text_codex_account() -> state::CodexAccountActivitySummary {
	state::CodexAccountActivitySummary {
		account_fingerprint: String::from("...acct01"),
		email: Some(String::from("primary@example.com")),
		plan_type: Some(String::from("pro")),
		status: String::from("selected"),
		refresh_status: String::from("not_needed"),
		checked_at_unix_epoch: Some(1_742_000_000),
		selected_at_unix_epoch: Some(1_742_000_001),
		primary_window_seconds: Some(18_000),
		primary_remaining_percent: Some(72),
		primary_resets_at_unix_epoch: Some(1_742_018_000),
		secondary_window_seconds: Some(604_800),
		secondary_remaining_percent: Some(91),
		secondary_resets_at_unix_epoch: Some(1_742_604_800),
		credits_has_credits: Some(true),
		credits_unlimited: Some(false),
		credits_balance: Some(String::from("9.99")),
		rate_limit_reached_type: None,
		cooldown_until_unix_epoch: None,
		note: Some(String::from("usage probe ok")),
		..state::CodexAccountActivitySummary::default()
	}
}

pub(super) fn operator_status_text_backup_codex_account() -> state::CodexAccountActivitySummary {
	state::CodexAccountActivitySummary {
		account_fingerprint: String::from("...acct02"),
		email: Some(String::from("backup@example.com")),
		plan_type: Some(String::from("plus")),
		status: String::from("available"),
		refresh_status: String::from("not_needed"),
		checked_at_unix_epoch: Some(1_742_000_002),
		selected_at_unix_epoch: None,
		primary_window_seconds: Some(18_000),
		primary_remaining_percent: Some(41),
		primary_resets_at_unix_epoch: Some(1_742_010_000),
		secondary_window_seconds: Some(604_800),
		secondary_remaining_percent: Some(88),
		secondary_resets_at_unix_epoch: Some(1_742_590_000),
		credits_has_credits: Some(true),
		credits_unlimited: Some(false),
		credits_balance: Some(String::from("4.20")),
		rate_limit_reached_type: None,
		cooldown_until_unix_epoch: None,
		note: Some(String::from("usage probe ok")),
		..state::CodexAccountActivitySummary::default()
	}
}

pub(super) fn operator_status_text_control_capability() -> OperatorRunControlCapability {
	OperatorRunControlCapability {
		project_id: String::from("pubfi"),
		issue_id: String::from("issue-1"),
		run_id: String::from("run-1"),
		attempt_number: 1,
		thread_id: Some(String::from("thread-1")),
		turn_id: Some(String::from("turn-1")),
		transport: String::from("local_file"),
		channel_path: String::from(".worktrees/PUB-101/.decodex-run-control/run-1-1.channel"),
		status: String::from("active"),
		published_at: String::from("2026-03-14 10:00:00"),
		updated_at: String::from("2026-03-14 10:00:01"),
	}
}

pub(super) fn operator_status_text_child_agent_activity() -> ChildAgentActivitySummary {
	ChildAgentActivitySummary {
		buckets: vec![
			ChildAgentActivityBucket {
				name: String::from("Model"),
				wall_seconds: 693,
				event_count: 12,
				tool_call_count: 0,
				input_tokens: 4_270_000,
				output_tokens: 12_000,
				output_bytes: 0,
			},
			ChildAgentActivityBucket {
				name: String::from("Browser/Image"),
				wall_seconds: 41,
				event_count: 6,
				tool_call_count: 3,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 180_000,
			},
		],
		current_bucket: Some(String::from("Model")),
		current_detail: Some(String::from("waiting after tool output")),
		current_started_unix_epoch: None,
		current_elapsed_seconds: Some(652),
		wall_seconds: 734,
		event_count: 18,
		tool_call_count: 3,
		input_tokens_current: Some(105_000),
		input_tokens_max: Some(105_000),
		input_tokens_cumulative: 4_270_000,
		output_tokens_cumulative: 12_000,
		largest_tool_output_bytes: Some(180_000),
		largest_tool_output_tool: Some(String::from("view_image")),
		large_output_warnings: vec![String::from(
			"view_image repeated 3 large outputs; largest 180000 bytes",
		)],
	}
}

pub(super) fn operator_status_text_protocol_activity() -> ProtocolActivitySummary {
	ProtocolActivitySummary {
		turn_status: Some(String::from("completed")),
		waiting_reason: Some(String::from("model_execution")),
		rate_limit_status: Some(String::from("none")),
		recent_events: vec![
			ProtocolActivityEventSummary {
				event_type: String::from("item/tool/call"),
				category: String::from("item"),
				detail: Some(String::from("view_image")),
			},
			ProtocolActivityEventSummary {
				event_type: String::from("turn/completed"),
				category: String::from("turn"),
				detail: Some(String::from("completed")),
			},
		],
	}
}

pub(super) fn test_worktree_provenance(source: &str) -> OperatorWorktreeProvenanceStatus {
	OperatorWorktreeProvenanceStatus {
		source: source.to_owned(),
		created_at_unix: Some(1),
		updated_at_unix: Some(2),
		audit_required: false,
	}
}

pub(in crate::orchestrator::tests::operator) fn operator_status_text_current_lane()
-> OperatorRunStatus {
	let account = operator_status_text_codex_account();
	let backup_account = operator_status_text_backup_codex_account();

	OperatorRunStatus {
		project_id: String::from("pubfi"),
		project_display_name: String::from("hack-ink/pubfi-mono-v2"),
		run_id: String::from("run-1"),
		issue_id: String::from("issue-1"),
		issue_identifier: Some(String::from("PUB-101")),
		title: Some(String::from("Implement orchestration")),
		author: Some(String::from("Yvette")),
		issue_state: None,
		active_label_present: None,
		needs_attention_label_present: None,
		attempt_number: 1,
		status: String::from("running"),
		attempt_status: String::from("running"),
		status_projection_reason: None,
		ownership_state: String::from("leased_run"),
		liveness_state: String::from("process_alive"),
		policy_state: String::from("allowed"),
		terminalization_state: String::from("none"),
		lane_control_next_action: String::from("continue_owned_attempt"),
		lane_control_conditions: Vec::new(),
		phase: String::from("executing"),
		run_phase: String::from("executing"),
		wait_reason: None,
		current_operation: String::from(RUN_OPERATION_AGENT_RUN),
		active_goal_phase: Some(String::from("implement_to_validation_ready")),
		public_progress_phase: Some(String::from("implementing")),
		thread_id: Some(String::from("thread-1")),
		turn_id: Some(String::from("turn-1")),
		thread_status: Some(String::from("active")),
		thread_active_flags: vec![String::from("waitingOnApproval")],
		interactive_requested: true,
		continuation_pending: false,
		continuation_recovery: None,
		phase_acceptance: None,
		run_lease: true,
		queue_lease_state: String::from("held"),
		execution_liveness: String::from("process_alive"),
		has_fresh_execution: true,
		counts_as_running: true,
		needs_attention: false,
		updated_at: String::from("2026-03-14 09:00:00"),
		last_run_activity_at: Some(String::from("2026-03-14 10:00:00Z")),
		last_protocol_activity_at: Some(String::from("2026-03-14 10:00:01Z")),
		last_progress_at: Some(String::from("2026-03-14 10:00:01Z")),
		idle_for_seconds: Some(1),
		protocol_idle_for_seconds: Some(1),
		suspected_stall: false,
		progress_diagnostic: None,
		last_event_type: Some(String::from("turn/completed")),
		last_event_at: Some(String::from("2026-03-14 10:00:01")),
		event_count: 4,
		private_evidence: AgentPrivateEvidenceRef {
			evidence_ref: String::from("private-evidence:pubfi/issue-1/run-1/1"),
			source: String::from("runtime_sqlite"),
			default_view: String::from("summarized_payloads"),
			read_command: String::from(
				"decodex evidence --config project.toml PUB-101 --run-id run-1 --attempt 1 --json",
			),
		},
		loop_status: None,
		control_capability: Some(operator_status_text_control_capability()),
		process_id: Some(1_234),
		process_alive: Some(true),
		process_liveness_reason: Some(String::from("process_alive")),
		retry_kind: None,
		next_retry_at: None,
		effective_model: Some(String::from("gpt-5.4")),
		effective_model_provider: Some(String::from("openai")),
		effective_cwd: Some(String::from("/tmp/worktree")),
		effective_approval_policy: Some(String::from("never")),
		effective_approvals_reviewer: Some(String::from("human")),
		effective_sandbox_mode: Some(String::from("workspaceWrite")),
		child_agent_activity: Some(operator_status_text_child_agent_activity()),
		protocol_activity: Some(operator_status_text_protocol_activity()),
		lifecycle_source: String::from("recorded"),
		lifecycle_evidence: vec![String::from("run_attempt")],
		lifecycle_gaps: Vec::new(),
		lifecycle_metrics: OperatorLaneLifecycleMetrics::default(),
		account: Some(account.clone()),
		accounts: vec![account, backup_account],
		branch_name: Some(String::from("x/pubfi-pub-101")),
		worktree_path: Some(String::from(".worktrees/PUB-101")),
	}
}

pub(in crate::orchestrator::tests::operator) fn operator_status_text_queued_candidates()
-> Vec<OperatorQueuedIssueStatus> {
	vec![
		OperatorQueuedIssueStatus {
			project_id: String::from(TEST_SERVICE_ID),
			issue_id: String::from("issue-1"),
			issue_identifier: String::from("PUB-101"),
			title: String::from("Running lane still has a backlog claim"),
			author: Some(String::from("Yvette")),
			state: String::from("In Progress"),
			priority: Some(1),
			created_at: String::from("2026-03-14T09:57:00Z"),
			classification: String::from("claimed"),
			reason: String::from("shared_claim_present"),
			attention: None,
			blocker_identifiers: vec![],
		},
		OperatorQueuedIssueStatus {
			project_id: String::from(TEST_SERVICE_ID),
			issue_id: String::from("issue-2"),
			issue_identifier: String::from("PUB-102"),
			title: String::from("Implement backlog surface"),
			author: Some(String::from("Yvette")),
			state: String::from("Todo"),
			priority: Some(2),
			created_at: String::from("2026-03-14T09:58:00Z"),
			classification: String::from("ready"),
			reason: String::from("eligible_for_dispatch"),
			attention: None,
			blocker_identifiers: vec![],
		},
		OperatorQueuedIssueStatus {
			project_id: String::from(TEST_SERVICE_ID),
			issue_id: String::from("issue-5"),
			issue_identifier: String::from("PUB-105"),
			title: String::from("Remove stale queue label"),
			author: Some(String::from("Yvette")),
			state: String::from("Done"),
			priority: Some(3),
			created_at: String::from("2026-03-14T09:59:00Z"),
			classification: String::from("closed"),
			reason: String::from("terminal_state"),
			attention: None,
			blocker_identifiers: vec![],
		},
	]
}

pub(in crate::orchestrator::tests::operator) fn operator_status_text_worktrees()
-> Vec<OperatorWorktreeStatus> {
	vec![
		OperatorWorktreeStatus {
			project_id: String::from(TEST_SERVICE_ID),
			issue_id: String::from("issue-4"),
			issue_identifier: Some(String::from("PUB-104")),
			issue_state: None,
			branch_name: String::from("x/pubfi-pub-104"),
			worktree_path: String::from(".worktrees/PUB-104"),
			ownership: String::from("cleanup_only"),
			ownership_reason: String::from(
				"No current lane, queued recovery, or post-review lane owns this worktree; local cleanup only.",
			),
			provenance: test_worktree_provenance("runtime_recorded"),
			recovery_next_action: None,
			hygiene: None,
		},
		OperatorWorktreeStatus {
			project_id: String::from(TEST_SERVICE_ID),
			issue_id: String::from("issue-1"),
			issue_identifier: Some(String::from("PUB-101")),
			issue_state: Some(String::from("In Progress")),
			branch_name: String::from("x/pubfi-pub-101"),
			worktree_path: String::from(".worktrees/PUB-101"),
			ownership: String::from("current_lane"),
			ownership_reason: String::from("Current lane `run-1` owns this worktree."),
			provenance: test_worktree_provenance("runtime_recorded"),
			recovery_next_action: None,
			hygiene: None,
		},
		OperatorWorktreeStatus {
			project_id: String::from(TEST_SERVICE_ID),
			issue_id: String::from("issue-3"),
			issue_identifier: Some(String::from("PUB-103")),
			issue_state: Some(String::from("In Review")),
			branch_name: String::from("x/pubfi-pub-103"),
			worktree_path: String::from(".worktrees/PUB-103"),
			ownership: String::from("post_review_lane"),
			ownership_reason: String::from(
				"Review & Landing owns this worktree as `ready_to_land`.",
			),
			provenance: test_worktree_provenance("runtime_recorded"),
			recovery_next_action: None,
			hygiene: None,
		},
	]
}

pub(in crate::orchestrator::tests::operator) fn operator_status_text_post_review_lanes()
-> Vec<OperatorPostReviewLaneStatus> {
	vec![OperatorPostReviewLaneStatus {
		project_id: String::from(TEST_SERVICE_ID),
		issue_id: String::from("issue-3"),
		issue_identifier: String::from("PUB-103"),
		issue_state: String::from("In Review"),
		branch_name: String::from("x/pubfi-pub-103"),
		worktree_path: String::from(".worktrees/PUB-103"),
		classification: String::from("ready_to_land"),
		reason: String::from("checks_green"),
		pr_url: Some(String::from("https://github.com/hack-ink/decodex/pull/103")),
		pr_head_sha: Some(String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6")),
		pr_state: Some(String::from("OPEN")),
		review_decision: Some(String::from("APPROVED")),
		mergeable: Some(String::from("MERGEABLE")),
		check_state: Some(String::from("SUCCESS")),
		unresolved_review_threads: Some(0),
		shadowed_by_current_lane: false,
		readback_warning: None,
		readback_root_cause: None,
		loop_status: None,
	}]
}

pub(in crate::orchestrator::tests::operator) fn assert_recovery_worktree_roles_are_grouped(
	rendered: &str,
) {
	let post_review_role_index =
		rendered.find("role: post_review_lane").expect("post-review role should render");
	let recovery_role_index =
		rendered.find("role: cleanup_only").expect("recovery role should render");

	assert!(post_review_role_index < recovery_role_index);
}
