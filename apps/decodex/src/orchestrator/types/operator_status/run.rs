use serde::{Deserialize, Serialize};

use crate::orchestrator::agent_evidence::AgentPrivateEvidenceRef;
use crate::orchestrator::types::operator_status::{
	lifecycle::OperatorLaneLifecycleMetrics, loop_status::OperatorLoopStatus,
};
use crate::state::{
	ChildAgentActivitySummary, CodexAccountActivitySummary, ProtocolActivitySummary,
};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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
