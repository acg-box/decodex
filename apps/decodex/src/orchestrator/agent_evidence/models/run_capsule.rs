use serde::Serialize;

use crate::orchestrator::agent_evidence::models::AgentPrivateEvidenceRef;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentRunCapsuleRef {
	pub(crate) evidence_ref: String,
	pub(crate) run_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) attempt_number: i64,
	pub(crate) status: String,
	pub(crate) phase: String,
	pub(crate) current_operation: String,
	pub(crate) path: String,
	pub(crate) private_evidence: AgentPrivateEvidenceRef,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentRunCapsule {
	pub(crate) schema: &'static str,
	pub(crate) evidence_ref: String,
	pub(crate) project_id: String,
	pub(crate) generated_at: String,
	pub(crate) path: String,
	pub(crate) run_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) title: Option<String>,
	pub(crate) attempt_number: i64,
	pub(crate) status: String,
	pub(crate) attempt_status: String,
	pub(crate) phase: String,
	pub(crate) wait_reason: Option<String>,
	pub(crate) current_operation: String,
	pub(crate) queue_lease_state: String,
	pub(crate) execution_liveness: String,
	pub(crate) ownership_state: String,
	pub(crate) liveness_state: String,
	pub(crate) policy_state: String,
	pub(crate) terminalization_state: String,
	pub(crate) lane_control_next_action: String,
	pub(crate) lane_control_conditions: Vec<String>,
	pub(crate) run_lease: bool,
	pub(crate) continuation_pending: bool,
	pub(crate) suspected_stall: bool,
	pub(crate) thread_id: Option<String>,
	pub(crate) turn_id: Option<String>,
	pub(crate) thread_status: Option<String>,
	pub(crate) thread_active_flags: Vec<String>,
	pub(crate) interactive_requested: bool,
	pub(crate) process_id: Option<u32>,
	pub(crate) process_alive: Option<bool>,
	pub(crate) process_liveness_reason: Option<String>,
	pub(crate) event_count: i64,
	pub(crate) last_event_type: Option<String>,
	pub(crate) last_event_at: Option<String>,
	pub(crate) last_run_activity_at: Option<String>,
	pub(crate) last_protocol_activity_at: Option<String>,
	pub(crate) last_progress_at: Option<String>,
	pub(crate) idle_for_seconds: Option<i64>,
	pub(crate) protocol_idle_for_seconds: Option<i64>,
	pub(crate) retry_kind: Option<String>,
	pub(crate) next_retry_at: Option<String>,
	pub(crate) effective_model: Option<String>,
	pub(crate) effective_model_provider: Option<String>,
	pub(crate) effective_cwd: Option<String>,
	pub(crate) effective_approval_policy: Option<String>,
	pub(crate) effective_approvals_reviewer: Option<String>,
	pub(crate) effective_sandbox_mode: Option<String>,
	pub(crate) branch_name: Option<String>,
	pub(crate) worktree_path: Option<String>,
	pub(crate) private_evidence: AgentPrivateEvidenceRef,
	pub(crate) ledger_outcome: Option<AgentRunLedgerOutcome>,
	pub(crate) diagnosis: AgentRunDiagnosis,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentRunLedgerOutcome {
	pub(crate) ledger_status: String,
	pub(crate) final_outcome: String,
	pub(crate) final_event_type: Option<String>,
	pub(crate) final_event_at: Option<String>,
	pub(crate) summary: Option<String>,
	pub(crate) pr_url: Option<String>,
	pub(crate) commit_sha: Option<String>,
	pub(crate) closeout_status: Option<String>,
	pub(crate) needs_attention_reason: Option<String>,
	pub(crate) record_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentRunDiagnosis {
	pub(crate) attention_required: bool,
	pub(crate) reason_code: Option<String>,
	pub(crate) next_action: Option<String>,
}
