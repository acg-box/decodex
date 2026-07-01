mod execution_program;

use super::{
	AgentPrivateEvidenceRef, ChildAgentActivityBucket, ChildAgentActivitySummary,
	CodexAccountActivitySummary, Deserialize, PostReviewLaneDecision, ProtocolActivitySummary,
	ReviewHandoffMarker, Serialize, TrackerIssue, WorktreeMapping,
};

pub(crate) use self::execution_program::{
	OperatorExecutionProgramNodeStatus, OperatorExecutionProgramStatus,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct OperatorStatusSnapshot {
	pub(crate) project_id: String,
	pub(crate) run_limit: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) status_source: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) snapshot_age_seconds: Option<i64>,
	pub(crate) warnings: Vec<String>,
	pub(crate) warning_details: Vec<OperatorSnapshotWarningDetail>,
	pub(crate) connector_backoffs: Vec<OperatorConnectorBackoffStatus>,
	pub(crate) projects: Vec<OperatorProjectStatus>,
	pub(crate) account_control: OperatorCodexAccountControlStatus,
	pub(crate) accounts: Vec<CodexAccountActivitySummary>,
	pub(crate) current_lanes: Vec<OperatorRunStatus>,
	pub(crate) recent_runs: Vec<OperatorRunStatus>,
	pub(crate) history_lanes: Vec<OperatorHistoryLaneStatus>,
	pub(crate) execution_programs: Vec<OperatorExecutionProgramStatus>,
	pub(crate) queued_candidates: Vec<OperatorQueuedIssueStatus>,
	pub(crate) worktrees: Vec<OperatorWorktreeStatus>,
	pub(crate) post_review_lanes: Vec<OperatorPostReviewLaneStatus>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorSnapshotWarningDetail {
	pub(crate) warning: String,
	pub(crate) project_id: Option<String>,
	pub(crate) repo_root: Option<String>,
	pub(crate) reason: String,
	pub(crate) next_action: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorConnectorBackoffStatus {
	pub(crate) project_id: String,
	pub(crate) connector: String,
	pub(crate) sync_phase: String,
	pub(crate) quota_class: String,
	pub(crate) reset_at: String,
	pub(crate) reset_unix_epoch: i64,
	pub(crate) reset_source: String,
	pub(crate) retry_after_seconds: i64,
	pub(crate) next_action: String,
	pub(crate) warning: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorProjectStatus {
	pub(crate) project_id: String,
	pub(crate) config_path: String,
	pub(crate) repo_root: String,
	pub(crate) enabled: bool,
	pub(crate) github_cli_authority: OperatorGitHubCliAuthority,
	pub(crate) current_lane_count: usize,
	pub(crate) running_lane_count: usize,
	pub(crate) queued_candidate_count: usize,
	pub(crate) post_review_lane_count: usize,
	pub(crate) retained_worktree_count: usize,
	pub(crate) waiting_lane_count: usize,
	pub(crate) attention_count: usize,
	pub(crate) cleanup_blocked_count: usize,
	pub(crate) cleanup_pending_count: usize,
	pub(crate) connector_state: String,
	pub(crate) last_activity_at: Option<String>,
	pub(crate) warning_count: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorGitHubCliAuthority {
	pub(crate) command_path: String,
	pub(crate) resolved_path: Option<String>,
	pub(crate) configured_path: Option<String>,
	pub(crate) discovery_tier: String,
	pub(crate) available: bool,
	pub(crate) next_action: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorCodexAccountControlStatus {
	pub(crate) mode: String,
	pub(crate) account_selector: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorHistoryLaneStatus {
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) title: Option<String>,
	pub(crate) author: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) issue_state: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) active_label_present: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) needs_attention_label_present: Option<bool>,
	pub(crate) issue_key: String,
	pub(crate) attempt_count: usize,
	pub(crate) ledger_outcome: OperatorHistoryLedgerOutcome,
	pub(crate) lifecycle_metrics: OperatorLaneLifecycleMetrics,
	pub(crate) latest_run: OperatorRunStatus,
	pub(crate) attempts: Vec<OperatorRunStatus>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorLaneLifecycleMetrics {
	pub(crate) attempt_count: usize,
	pub(crate) run_count: usize,
	pub(crate) recorded_attempt_count: usize,
	pub(crate) recovered_attempt_count: usize,
	pub(crate) current_snapshot_attempt_count: usize,
	pub(crate) captured_attempt_count: usize,
	pub(crate) missing_attempt_count: usize,
	pub(crate) protocol_event_count: i64,
	pub(crate) child_event_count: i64,
	pub(crate) wall_seconds: i64,
	pub(crate) tool_call_count: i64,
	pub(crate) input_tokens_current: Option<i64>,
	pub(crate) input_tokens_peak: Option<i64>,
	pub(crate) input_tokens_cumulative: i64,
	pub(crate) output_tokens_cumulative: i64,
	pub(crate) largest_tool_output_bytes: Option<i64>,
	pub(crate) largest_tool_output_tool: Option<String>,
	pub(crate) large_output_warnings: Vec<String>,
	pub(crate) buckets: Vec<ChildAgentActivityBucket>,
	pub(crate) phases: Vec<OperatorLaneLifecyclePhaseMetrics>,
	pub(crate) attempt_evidence: Vec<OperatorLaneLifecycleAttemptEvidence>,
	pub(crate) recovery_gaps: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorLaneLifecyclePhaseMetrics {
	pub(crate) phase: String,
	pub(crate) label: String,
	pub(crate) attempt_count: usize,
	pub(crate) run_count: usize,
	pub(crate) recorded_attempt_count: usize,
	pub(crate) recovered_attempt_count: usize,
	pub(crate) current_snapshot_attempt_count: usize,
	pub(crate) captured_attempt_count: usize,
	pub(crate) missing_attempt_count: usize,
	pub(crate) protocol_event_count: i64,
	pub(crate) child_event_count: i64,
	pub(crate) wall_seconds: i64,
	pub(crate) tool_call_count: i64,
	pub(crate) input_tokens_current: Option<i64>,
	pub(crate) input_tokens_peak: Option<i64>,
	pub(crate) input_tokens_cumulative: i64,
	pub(crate) output_tokens_cumulative: i64,
	pub(crate) largest_tool_output_bytes: Option<i64>,
	pub(crate) largest_tool_output_tool: Option<String>,
	pub(crate) large_output_warnings: Vec<String>,
	pub(crate) buckets: Vec<ChildAgentActivityBucket>,
	pub(crate) attempt_evidence: Vec<OperatorLaneLifecycleAttemptEvidence>,
	pub(crate) recovery_gaps: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorLaneLifecycleAttemptEvidence {
	pub(crate) run_id: String,
	pub(crate) issue_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) status: String,
	pub(crate) phase: String,
	pub(crate) source: String,
	pub(crate) evidence: Vec<String>,
	pub(crate) gaps: Vec<String>,
	pub(crate) protocol_event_count: i64,
	pub(crate) child_event_count: i64,
	pub(crate) updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorHistoryLedgerOutcome {
	pub(crate) ledger_status: String,
	pub(crate) final_outcome: String,
	pub(crate) final_event_type: Option<String>,
	pub(crate) final_event_at: Option<String>,
	pub(crate) summary: Option<String>,
	pub(crate) pr_url: Option<String>,
	pub(crate) commit_sha: Option<String>,
	pub(crate) branch: Option<String>,
	pub(crate) closeout_status: Option<String>,
	pub(crate) needs_attention_reason: Option<String>,
	pub(crate) lifecycle_started_at: Option<String>,
	pub(crate) lifecycle_finished_at: Option<String>,
	pub(crate) lifecycle_elapsed_seconds: Option<i64>,
	pub(crate) record_count: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorRunStatus {
	pub(crate) project_id: String,
	pub(crate) project_display_name: String,
	pub(crate) run_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) title: Option<String>,
	pub(crate) author: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) issue_state: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) active_label_present: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) needs_attention_label_present: Option<bool>,
	pub(crate) attempt_number: i64,
	pub(crate) status: String,
	pub(crate) attempt_status: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) status_projection_reason: Option<String>,
	pub(crate) ownership_state: String,
	pub(crate) liveness_state: String,
	pub(crate) policy_state: String,
	pub(crate) terminalization_state: String,
	pub(crate) lane_control_next_action: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) lane_control_conditions: Vec<String>,
	pub(crate) phase: String,
	#[serde(default)]
	pub(crate) run_phase: String,
	pub(crate) wait_reason: Option<String>,
	pub(crate) current_operation: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) active_goal_phase: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) public_progress_phase: Option<String>,
	pub(crate) thread_id: Option<String>,
	pub(crate) turn_id: Option<String>,
	pub(crate) thread_status: Option<String>,
	pub(crate) thread_active_flags: Vec<String>,
	pub(crate) interactive_requested: bool,
	pub(crate) continuation_pending: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) continuation_recovery: Option<OperatorContinuationRecoveryStatus>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) phase_acceptance: Option<OperatorPhaseAcceptanceStatus>,
	pub(crate) run_lease: bool,
	pub(crate) queue_lease_state: String,
	pub(crate) execution_liveness: String,
	pub(crate) has_fresh_execution: bool,
	pub(crate) counts_as_running: bool,
	pub(crate) needs_attention: bool,
	pub(crate) updated_at: String,
	pub(crate) last_run_activity_at: Option<String>,
	pub(crate) last_protocol_activity_at: Option<String>,
	pub(crate) last_progress_at: Option<String>,
	pub(crate) idle_for_seconds: Option<i64>,
	pub(crate) protocol_idle_for_seconds: Option<i64>,
	pub(crate) suspected_stall: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) progress_diagnostic: Option<String>,
	pub(crate) last_event_type: Option<String>,
	pub(crate) last_event_at: Option<String>,
	pub(crate) event_count: i64,
	pub(in crate::orchestrator) private_evidence: AgentPrivateEvidenceRef,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) loop_status: Option<OperatorLoopStatus>,
	pub(crate) control_capability: Option<OperatorRunControlCapability>,
	pub(crate) process_id: Option<u32>,
	pub(crate) process_alive: Option<bool>,
	pub(crate) process_liveness_reason: Option<String>,
	pub(crate) retry_kind: Option<String>,
	pub(crate) next_retry_at: Option<String>,
	pub(crate) effective_model: Option<String>,
	pub(crate) effective_model_provider: Option<String>,
	pub(crate) effective_cwd: Option<String>,
	pub(crate) effective_approval_policy: Option<String>,
	pub(crate) effective_approvals_reviewer: Option<String>,
	pub(crate) effective_sandbox_mode: Option<String>,
	pub(crate) child_agent_activity: Option<ChildAgentActivitySummary>,
	pub(crate) protocol_activity: Option<ProtocolActivitySummary>,
	pub(crate) lifecycle_source: String,
	pub(crate) lifecycle_evidence: Vec<String>,
	pub(crate) lifecycle_gaps: Vec<String>,
	#[serde(default)]
	pub(crate) lifecycle_metrics: OperatorLaneLifecycleMetrics,
	pub(crate) account: Option<CodexAccountActivitySummary>,
	pub(crate) accounts: Vec<CodexAccountActivitySummary>,
	pub(crate) branch_name: Option<String>,
	pub(crate) worktree_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorContinuationRecoveryStatus {
	pub(crate) state: String,
	pub(crate) source_phase: String,
	pub(crate) next_phase: String,
	pub(crate) source_error_class: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) source_error_message: Option<String>,
	pub(crate) recorded_at: String,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) recovery_count: i64,
	pub(crate) automatic_continuation_limit: i64,
	pub(crate) budget_exceeded: bool,
	pub(crate) next_action: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorPhaseAcceptanceStatus {
	pub(crate) phase: String,
	pub(crate) decision: String,
	pub(crate) reason_code: String,
	pub(crate) objective_covered: bool,
	pub(crate) effective_delta_present: bool,
	pub(crate) changed_surfaces: Vec<String>,
	pub(crate) non_goal_passed: bool,
	pub(crate) validation_passed: bool,
	pub(crate) recorded_at: String,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) next_action: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorRunControlCapability {
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) thread_id: Option<String>,
	pub(crate) turn_id: Option<String>,
	pub(crate) transport: String,
	pub(crate) channel_path: String,
	pub(crate) status: String,
	pub(crate) published_at: String,
	pub(crate) updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorQueuedIssueStatus {
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: String,
	pub(crate) title: String,
	pub(crate) author: Option<String>,
	pub(crate) state: String,
	pub(crate) priority: Option<i64>,
	pub(crate) created_at: String,
	pub(crate) classification: String,
	pub(crate) reason: String,
	pub(crate) attention: Option<OperatorQueuedIssueAttentionStatus>,
	pub(crate) blocker_identifiers: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorQueuedIssueAttentionStatus {
	pub(crate) summary: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) decision_request: Option<OperatorAuthorityDecisionRequestStatus>,
	pub(crate) run_id: Option<String>,
	pub(crate) attempt_number: Option<i64>,
	pub(crate) current_operation: Option<String>,
	pub(crate) thread_status: Option<String>,
	pub(crate) attempt_status: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) loop_status: Option<OperatorLoopStatus>,
	pub(crate) auto_retry_blocked_reason: Option<String>,
	pub(crate) attention_error_class: Option<String>,
	pub(crate) attention_next_action: Option<String>,
	pub(crate) retry_budget_attempt_count: Option<i64>,
	pub(crate) retry_budget_max_attempts: i64,
	pub(crate) last_activity_at: Option<String>,
	pub(crate) last_progress_at: Option<String>,
	pub(crate) last_event_type: Option<String>,
	pub(crate) event_count: i64,
	pub(crate) process_alive: Option<bool>,
	pub(crate) process_liveness_reason: Option<String>,
	pub(crate) worktree_path: Option<String>,
	pub(crate) worktree_has_tracked_changes: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorAuthorityDecisionRequestStatus {
	pub(crate) phase: String,
	pub(crate) reason: String,
	pub(crate) boundary: String,
	pub(crate) decision_request_id: String,
	pub(crate) next_action: String,
	pub(crate) recommendation: Option<String>,
	pub(crate) resume_condition: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorLoopStatus {
	pub(crate) review_level: String,
	pub(crate) autonomy: String,
	pub(crate) summary: String,
	pub(crate) next_action: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) autonomy_objective: Option<OperatorAutonomyObjectiveStatus>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) autonomy_signals: Vec<OperatorAutonomySignalStatus>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) autonomy_proposals: Vec<OperatorAutonomyProposalStatus>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) autonomy_lineage: Vec<OperatorAutonomyLineageStatus>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) autonomy_report: Option<OperatorAutonomyReportReadbackStatus>,
	pub(crate) review: Option<OperatorReviewLoopStatus>,
	pub(crate) architecture_recovery: Option<OperatorArchitectureRecoveryStatus>,
	pub(crate) boundary: Option<OperatorBoundaryStatus>,
	pub(crate) decision_request: Option<OperatorAuthorityDecisionRequestStatus>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorAutonomyObjectiveStatus {
	pub(crate) objective_id: String,
	pub(crate) objective_version: u64,
	pub(crate) state: String,
	pub(crate) summary: String,
	pub(crate) source_ref: String,
	pub(crate) updated_at: String,
	pub(crate) completeness: String,
	pub(crate) known_gaps: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorAutonomySignalStatus {
	pub(crate) signal_id: String,
	pub(crate) objective_id: String,
	pub(crate) objective_version: u64,
	pub(crate) kind: String,
	pub(crate) source_type: String,
	pub(crate) source_refs: Vec<String>,
	pub(crate) primary_source_refs: Vec<String>,
	pub(crate) freshness: String,
	pub(crate) evidence_class: String,
	pub(crate) confidence: String,
	pub(crate) privacy: String,
	pub(crate) redaction_level: String,
	pub(crate) completeness: String,
	pub(crate) gaps: Vec<String>,
	pub(crate) known_gaps: Vec<String>,
	pub(crate) contradictions: Vec<String>,
	pub(crate) updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorAutonomyProposalStatus {
	pub(crate) proposal_id: String,
	pub(crate) objective_id: String,
	pub(crate) objective_version: u64,
	pub(crate) state: String,
	pub(crate) summary: String,
	pub(crate) source_family: String,
	pub(crate) intended_surface: String,
	pub(crate) affected_identifiers: Vec<String>,
	pub(crate) source_signal_ids: Vec<String>,
	pub(crate) refusal_reasons: Vec<String>,
	pub(crate) refusals: Vec<OperatorAutonomyProposalRefusalStatus>,
	pub(crate) completeness: String,
	pub(crate) known_gaps: Vec<String>,
	pub(crate) gaps: Vec<String>,
	pub(crate) contradictions: Vec<String>,
	pub(crate) challenge_evidence_count: usize,
	pub(crate) updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorAutonomyProposalRefusalStatus {
	pub(crate) reason: String,
	pub(crate) detail: String,
	pub(crate) evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorAutonomyLineageStatus {
	pub(crate) objective_ref: String,
	pub(crate) signal_ids: Vec<String>,
	pub(crate) proposal_id: Option<String>,
	pub(crate) proposal_state: Option<String>,
	pub(crate) decision_contracts: Vec<OperatorAutonomyDecisionContractStatus>,
	pub(crate) program_intake: Vec<OperatorAutonomyProgramIntakeStatus>,
	pub(crate) execution_evidence: Vec<OperatorAutonomyExecutionEvidenceStatus>,
	pub(crate) completeness: String,
	pub(crate) known_gaps: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorAutonomyDecisionContractStatus {
	pub(crate) contract_id: String,
	pub(crate) status: String,
	pub(crate) updated_at: String,
	pub(crate) generated_issue_identifiers: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorAutonomyProgramIntakeStatus {
	pub(crate) program_id: String,
	pub(crate) plan_id: String,
	pub(crate) intake_kind: String,
	pub(crate) source_contract_id: String,
	pub(crate) public_summary: String,
	pub(crate) updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorAutonomyExecutionEvidenceStatus {
	pub(crate) kind: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) source_refs: Vec<String>,
	pub(crate) summary: String,
	pub(crate) updated_at: String,
	pub(crate) completeness: String,
	pub(crate) known_gaps: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorAutonomyReportReadbackStatus {
	pub(crate) surface: String,
	pub(crate) authority: String,
	pub(crate) audit_authority: bool,
	pub(crate) source_refs: Vec<String>,
	pub(crate) redaction_level: String,
	pub(crate) completeness: String,
	pub(crate) known_gaps: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorReviewLoopStatus {
	pub(crate) phase: String,
	pub(crate) status: String,
	pub(crate) checkpoint: Option<OperatorReviewCheckpointStatus>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorReviewCheckpointStatus {
	pub(crate) head_sha: String,
	pub(crate) round: i64,
	pub(crate) nonclean_rounds: i64,
	pub(crate) review_class: Option<String>,
	pub(crate) risk_class: Option<String>,
	pub(crate) compact_eligible: Option<bool>,
	pub(crate) fallback_reason: Option<String>,
	pub(crate) active_fingerprints: Vec<String>,
	pub(crate) stop_fingerprint: Option<String>,
	pub(crate) route_counts: Vec<OperatorReviewRouteCount>,
	pub(crate) route_next_action: Option<String>,
	pub(crate) updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorReviewRouteCount {
	pub(crate) route: String,
	pub(crate) count: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorArchitectureRecoveryStatus {
	pub(crate) status: String,
	pub(crate) reason_code: String,
	pub(crate) guardrail_reason: Option<String>,
	pub(crate) boundary_disposition: Option<String>,
	pub(crate) boundary_policy_decision: Option<String>,
	pub(crate) requires_enhanced_evidence: bool,
	pub(crate) blocks_landing: bool,
	pub(crate) round: Option<u64>,
	pub(crate) budget: Option<OperatorRecoveryBudgetStatus>,
	pub(crate) next_action: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorRecoveryBudgetStatus {
	pub(crate) attempt: u64,
	pub(crate) max_attempts: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorBoundaryStatus {
	pub(crate) disposition: String,
	pub(crate) policy_decision: String,
	pub(crate) reason: Option<String>,
	pub(crate) attempted_recovery_reason: Option<String>,
	pub(crate) changed_surface_count: usize,
	pub(crate) improvement_signal_count: usize,
	pub(crate) requires_enhanced_evidence: bool,
	pub(crate) blocks_landing: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorWorktreeStatus {
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) issue_state: Option<String>,
	pub(crate) branch_name: String,
	pub(crate) worktree_path: String,
	pub(crate) ownership: String,
	pub(crate) ownership_reason: String,
	pub(crate) provenance: OperatorWorktreeProvenanceStatus,
	pub(crate) recovery_next_action: Option<String>,
	pub(crate) hygiene: Option<OperatorWorktreeHygieneStatus>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorWorktreeProvenanceStatus {
	pub(crate) source: String,
	pub(crate) created_at_unix: Option<i64>,
	pub(crate) updated_at_unix: Option<i64>,
	pub(crate) audit_required: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorWorktreeHygieneStatus {
	pub(crate) classification: String,
	pub(crate) default_branch: String,
	pub(crate) dirty: bool,
	pub(crate) reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorPostReviewLaneStatus {
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: String,
	pub(crate) issue_state: String,
	pub(crate) branch_name: String,
	pub(crate) worktree_path: String,
	pub(crate) classification: String,
	pub(crate) reason: String,
	pub(crate) pr_url: Option<String>,
	pub(crate) pr_head_sha: Option<String>,
	pub(crate) pr_state: Option<String>,
	pub(crate) review_decision: Option<String>,
	pub(crate) mergeable: Option<String>,
	pub(crate) check_state: Option<String>,
	pub(crate) unresolved_review_threads: Option<usize>,
	pub(crate) shadowed_by_current_lane: bool,
	pub(crate) readback_warning: Option<String>,
	pub(crate) readback_root_cause: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) loop_status: Option<OperatorLoopStatus>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PostReviewLaneSnapshot {
	pub(crate) issue: TrackerIssue,
	pub(crate) worktree: WorktreeMapping,
	pub(crate) review_handoff: Option<ReviewHandoffMarker>,
	pub(crate) local_branch_name: Option<String>,
	pub(crate) local_head_oid: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PostReviewLaneClassification {
	pub(crate) decision: PostReviewLaneDecision,
	pub(crate) reason: String,
	pub(crate) pr_url: Option<String>,
	pub(crate) pr_head_sha: Option<String>,
	pub(crate) pr_state: Option<String>,
	pub(crate) review_decision: Option<String>,
	pub(crate) mergeable: Option<String>,
	pub(crate) check_state: Option<String>,
	pub(crate) unresolved_review_threads: Option<usize>,
	pub(crate) readback_warning: Option<String>,
	pub(crate) readback_root_cause: Option<String>,
}

pub(crate) struct RetainedReviewLaneBlocked {
	pub(crate) issue: TrackerIssue,
	pub(crate) worktree: WorktreeMapping,
	pub(crate) run_identity: RetainedReviewRunIdentity,
	pub(crate) reason: String,
}

pub(crate) struct RetainedReviewRunIdentity {
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
}
