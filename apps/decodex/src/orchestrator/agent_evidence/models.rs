use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::orchestrator::{
	OperatorGitHubCliAuthority, harness_improvement::HarnessImprovementCandidateSummary,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) enum AgentEvidenceSource {
	DiagnoseCommand,
	ServeTick,
}
impl AgentEvidenceSource {
	pub(in crate::orchestrator) fn as_str(self) -> &'static str {
		match self {
			Self::DiagnoseCommand => "diagnose_command",
			Self::ServeTick => "serve_tick",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct AgentEvidenceWriteResult {
	pub(in crate::orchestrator) project_id: String,
	pub(in crate::orchestrator) handoff_index_path: String,
	pub(in crate::orchestrator) handoff_index: AgentHandoffIndex,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct AgentEvidenceSummary {
	pub(in crate::orchestrator) project_count: usize,
	pub(in crate::orchestrator) current_lane_count: usize,
	pub(in crate::orchestrator) recent_run_count: usize,
	pub(in crate::orchestrator) history_lane_count: usize,
	pub(in crate::orchestrator) queued_candidate_count: usize,
	pub(in crate::orchestrator) post_review_lane_count: usize,
	pub(in crate::orchestrator) recovery_worktree_count: usize,
	pub(in crate::orchestrator) blocker_count: usize,
	pub(in crate::orchestrator) run_capsule_count: usize,
	pub(in crate::orchestrator) connector_backoff_count: usize,
	pub(in crate::orchestrator) warning_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(in crate::orchestrator) struct AgentPrivateEvidenceRef {
	pub(in crate::orchestrator) evidence_ref: String,
	pub(in crate::orchestrator) source: String,
	pub(in crate::orchestrator) default_view: String,
	pub(in crate::orchestrator) read_command: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(in crate::orchestrator) struct PrivateEvidenceReadback {
	pub(in crate::orchestrator) schema: &'static str,
	pub(in crate::orchestrator) project_id: String,
	pub(in crate::orchestrator) issue_selector: String,
	pub(in crate::orchestrator) issue_id: String,
	pub(in crate::orchestrator) issue_identifier: Option<String>,
	pub(in crate::orchestrator) run_id: String,
	pub(in crate::orchestrator) attempt_number: i64,
	pub(in crate::orchestrator) source: &'static str,
	pub(in crate::orchestrator) evidence_ref: String,
	pub(in crate::orchestrator) read_command: String,
	pub(in crate::orchestrator) payload_mode: &'static str,
	pub(in crate::orchestrator) event_count: usize,
	pub(in crate::orchestrator) latest_event_type: Option<String>,
	pub(in crate::orchestrator) latest_event_at: Option<String>,
	pub(in crate::orchestrator) review_checkpoints: Vec<PrivateEvidenceReviewCheckpointSummary>,
	pub(in crate::orchestrator) repo_gate_failures: Vec<PrivateEvidenceRepoGateFailureSummary>,
	pub(in crate::orchestrator) phase_acceptance_checks: Vec<PrivateEvidencePhaseAcceptanceSummary>,
	pub(in crate::orchestrator) boundary_checks: Vec<PrivateEvidenceBoundaryCheckSummary>,
	pub(in crate::orchestrator) decision_requests: Vec<PrivateEvidenceDecisionRequestSummary>,
	pub(in crate::orchestrator) architecture_recoveries:
		Vec<PrivateEvidenceArchitectureRecoverySummary>,
	pub(in crate::orchestrator) improvement_candidates: Vec<HarnessImprovementCandidateSummary>,
	pub(in crate::orchestrator) events: Vec<PrivateEvidenceReadbackEvent>,
	pub(in crate::orchestrator) warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct PrivateEvidenceDecisionRequestSummary {
	pub(in crate::orchestrator) decision_request_id: String,
	pub(in crate::orchestrator) phase: String,
	pub(in crate::orchestrator) reason: String,
	pub(in crate::orchestrator) boundary: String,
	pub(in crate::orchestrator) next_action: String,
	pub(in crate::orchestrator) recommendation: Option<String>,
	pub(in crate::orchestrator) resume_condition: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct PrivateEvidenceReviewCheckpointSummary {
	pub(in crate::orchestrator) phase: String,
	pub(in crate::orchestrator) status: String,
	pub(in crate::orchestrator) head_sha: Option<String>,
	pub(in crate::orchestrator) round: Option<u64>,
	pub(in crate::orchestrator) review_class: Option<String>,
	pub(in crate::orchestrator) risk_class: Option<String>,
	pub(in crate::orchestrator) compact_eligible: Option<bool>,
	pub(in crate::orchestrator) fallback_reason: Option<String>,
	pub(in crate::orchestrator) active_fingerprints: Vec<String>,
	pub(in crate::orchestrator) stop_fingerprint: Option<String>,
	pub(in crate::orchestrator) accepted_finding_count: usize,
	pub(in crate::orchestrator) rejected_finding_count: usize,
	pub(in crate::orchestrator) route_counts: Vec<PrivateEvidenceReviewRouteCount>,
	pub(in crate::orchestrator) route_next_action: Option<String>,
	pub(in crate::orchestrator) next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct PrivateEvidenceReviewRouteCount {
	pub(in crate::orchestrator) route: String,
	pub(in crate::orchestrator) count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct PrivateEvidenceRepoGateFailureSummary {
	pub(in crate::orchestrator) record_id: i64,
	pub(in crate::orchestrator) phase: String,
	pub(in crate::orchestrator) error_class: String,
	pub(in crate::orchestrator) disposition: String,
	pub(in crate::orchestrator) stage: Option<String>,
	pub(in crate::orchestrator) failed_command: Option<String>,
	pub(in crate::orchestrator) exit_status: Option<i64>,
	pub(in crate::orchestrator) summary: Option<String>,
	pub(in crate::orchestrator) problem_lines: Vec<String>,
	pub(in crate::orchestrator) output_excerpt: Option<String>,
	pub(in crate::orchestrator) output_truncated: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct PrivateEvidencePhaseAcceptanceSummary {
	pub(in crate::orchestrator) phase: String,
	pub(in crate::orchestrator) decision: String,
	pub(in crate::orchestrator) reason_code: String,
	pub(in crate::orchestrator) objective_covered: bool,
	pub(in crate::orchestrator) effective_delta_present: bool,
	pub(in crate::orchestrator) changed_surfaces: Vec<String>,
	pub(in crate::orchestrator) non_goal_passed: bool,
	pub(in crate::orchestrator) validation_passed: bool,
	pub(in crate::orchestrator) next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct PrivateEvidenceBoundaryCheckSummary {
	pub(in crate::orchestrator) disposition: String,
	pub(in crate::orchestrator) policy_decision: String,
	pub(in crate::orchestrator) reason: Option<String>,
	pub(in crate::orchestrator) attempted_recovery_reason: Option<String>,
	pub(in crate::orchestrator) decision_contract_count: usize,
	pub(in crate::orchestrator) changed_surface_count: usize,
	pub(in crate::orchestrator) improvement_signal_count: usize,
	pub(in crate::orchestrator) requires_enhanced_evidence: bool,
	pub(in crate::orchestrator) blocks_landing: bool,
	pub(in crate::orchestrator) next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct PrivateEvidenceArchitectureRecoverySummary {
	pub(in crate::orchestrator) reason_code: String,
	pub(in crate::orchestrator) guardrail_reason: Option<String>,
	pub(in crate::orchestrator) boundary_disposition: Option<String>,
	pub(in crate::orchestrator) boundary_policy_decision: Option<String>,
	pub(in crate::orchestrator) requires_enhanced_evidence: bool,
	pub(in crate::orchestrator) blocks_landing: bool,
	pub(in crate::orchestrator) recovery_budget_attempt: Option<u64>,
	pub(in crate::orchestrator) recovery_budget_max_attempts: Option<u64>,
	pub(in crate::orchestrator) next_action: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(in crate::orchestrator) struct PrivateEvidenceReadbackEvent {
	pub(in crate::orchestrator) record_id: i64,
	pub(in crate::orchestrator) event_type: String,
	pub(in crate::orchestrator) recorded_at: String,
	pub(in crate::orchestrator) payload_summary: PrivateEvidencePayloadSummary,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::orchestrator) payload: Option<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct PrivateEvidencePayloadSummary {
	pub(in crate::orchestrator) kind: String,
	pub(in crate::orchestrator) byte_count: usize,
	pub(in crate::orchestrator) keys: Vec<String>,
	pub(in crate::orchestrator) preview: Vec<String>,
	pub(in crate::orchestrator) redacted_default_keys: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct AgentHandoffIndex {
	pub(in crate::orchestrator::agent_evidence) schema: &'static str,
	pub(in crate::orchestrator::agent_evidence) project_id: String,
	pub(in crate::orchestrator::agent_evidence) generated_at: String,
	pub(in crate::orchestrator::agent_evidence) source: String,
	pub(in crate::orchestrator::agent_evidence) evidence_root: String,
	pub(in crate::orchestrator::agent_evidence) handoff_index_path: String,
	pub(in crate::orchestrator::agent_evidence) blockers_dir: String,
	pub(in crate::orchestrator::agent_evidence) runs_dir: String,
	pub(in crate::orchestrator::agent_evidence) events_path: String,
	pub(in crate::orchestrator) summary: AgentEvidenceSummary,
	pub(in crate::orchestrator::agent_evidence) github_cli_authority:
		Option<OperatorGitHubCliAuthority>,
	pub(in crate::orchestrator::agent_evidence) warnings: Vec<String>,
	pub(in crate::orchestrator::agent_evidence) connector_backoffs: Vec<AgentConnectorBackoff>,
	pub(in crate::orchestrator::agent_evidence) blockers: Vec<AgentBlocker>,
	pub(in crate::orchestrator::agent_evidence) run_capsules: Vec<AgentRunCapsuleRef>,
	pub(in crate::orchestrator::agent_evidence) recovery_worktrees: Vec<AgentRecoveryWorktree>,
	pub(in crate::orchestrator::agent_evidence) recovery_contracts: Vec<AgentRecoveryContract>,
}

pub(in crate::orchestrator::agent_evidence) struct PrivateEvidenceTarget {
	pub(in crate::orchestrator::agent_evidence) issue_id: String,
	pub(in crate::orchestrator::agent_evidence) issue_identifier: Option<String>,
	pub(in crate::orchestrator::agent_evidence) run_id: String,
	pub(in crate::orchestrator::agent_evidence) attempt_number: i64,
}

pub(in crate::orchestrator::agent_evidence) struct AgentEvidenceFileWriteContext<'a> {
	pub(in crate::orchestrator::agent_evidence) project_id: &'a str,
	pub(in crate::orchestrator::agent_evidence) generated_at: &'a str,
	pub(in crate::orchestrator::agent_evidence) source: AgentEvidenceSource,
	pub(in crate::orchestrator::agent_evidence) handoff_index_path: &'a Path,
	pub(in crate::orchestrator::agent_evidence) blockers_dir: &'a Path,
	pub(in crate::orchestrator::agent_evidence) events_path: &'a Path,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator::agent_evidence) struct AgentConnectorBackoff {
	pub(in crate::orchestrator::agent_evidence) evidence_ref: String,
	pub(in crate::orchestrator::agent_evidence) connector: String,
	pub(in crate::orchestrator::agent_evidence) sync_phase: String,
	pub(in crate::orchestrator::agent_evidence) quota_class: String,
	pub(in crate::orchestrator::agent_evidence) reset_at: String,
	pub(in crate::orchestrator::agent_evidence) reset_unix_epoch: i64,
	pub(in crate::orchestrator::agent_evidence) reset_source: String,
	pub(in crate::orchestrator::agent_evidence) retry_after_seconds: i64,
	pub(in crate::orchestrator::agent_evidence) warning: String,
	pub(in crate::orchestrator::agent_evidence) next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator::agent_evidence) struct AgentBlocker {
	pub(in crate::orchestrator::agent_evidence) evidence_ref: String,
	pub(in crate::orchestrator::agent_evidence) project_id: String,
	pub(in crate::orchestrator::agent_evidence) surface: String,
	pub(in crate::orchestrator::agent_evidence) issue_id: Option<String>,
	pub(in crate::orchestrator::agent_evidence) issue_identifier: Option<String>,
	pub(in crate::orchestrator::agent_evidence) run_id: Option<String>,
	pub(in crate::orchestrator::agent_evidence) attempt_number: Option<i64>,
	pub(in crate::orchestrator::agent_evidence) classification: String,
	pub(in crate::orchestrator::agent_evidence) reason_code: String,
	pub(in crate::orchestrator::agent_evidence) reason: String,
	pub(in crate::orchestrator::agent_evidence) next_action: String,
	pub(in crate::orchestrator::agent_evidence) blocker_snapshot_path: String,
	pub(in crate::orchestrator::agent_evidence) related_run_capsule_path: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator::agent_evidence) struct AgentBlockerSnapshot {
	pub(in crate::orchestrator::agent_evidence) schema: &'static str,
	pub(in crate::orchestrator::agent_evidence) project_id: String,
	pub(in crate::orchestrator::agent_evidence) generated_at: String,
	pub(in crate::orchestrator::agent_evidence) issue_id: Option<String>,
	pub(in crate::orchestrator::agent_evidence) issue_identifier: Option<String>,
	pub(in crate::orchestrator::agent_evidence) blockers: Vec<AgentBlocker>,
	pub(in crate::orchestrator::agent_evidence) related_run_capsules: Vec<AgentRunCapsuleRef>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator::agent_evidence) struct AgentRunCapsuleRef {
	pub(in crate::orchestrator::agent_evidence) evidence_ref: String,
	pub(in crate::orchestrator::agent_evidence) run_id: String,
	pub(in crate::orchestrator::agent_evidence) issue_id: String,
	pub(in crate::orchestrator::agent_evidence) issue_identifier: Option<String>,
	pub(in crate::orchestrator::agent_evidence) attempt_number: i64,
	pub(in crate::orchestrator::agent_evidence) status: String,
	pub(in crate::orchestrator::agent_evidence) phase: String,
	pub(in crate::orchestrator::agent_evidence) current_operation: String,
	pub(in crate::orchestrator::agent_evidence) path: String,
	pub(in crate::orchestrator::agent_evidence) private_evidence: AgentPrivateEvidenceRef,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator::agent_evidence) struct AgentRunCapsule {
	pub(in crate::orchestrator::agent_evidence) schema: &'static str,
	pub(in crate::orchestrator::agent_evidence) evidence_ref: String,
	pub(in crate::orchestrator::agent_evidence) project_id: String,
	pub(in crate::orchestrator::agent_evidence) generated_at: String,
	pub(in crate::orchestrator::agent_evidence) path: String,
	pub(in crate::orchestrator::agent_evidence) run_id: String,
	pub(in crate::orchestrator::agent_evidence) issue_id: String,
	pub(in crate::orchestrator::agent_evidence) issue_identifier: Option<String>,
	pub(in crate::orchestrator::agent_evidence) title: Option<String>,
	pub(in crate::orchestrator::agent_evidence) attempt_number: i64,
	pub(in crate::orchestrator::agent_evidence) status: String,
	pub(in crate::orchestrator::agent_evidence) attempt_status: String,
	pub(in crate::orchestrator::agent_evidence) phase: String,
	pub(in crate::orchestrator::agent_evidence) wait_reason: Option<String>,
	pub(in crate::orchestrator::agent_evidence) current_operation: String,
	pub(in crate::orchestrator::agent_evidence) queue_lease_state: String,
	pub(in crate::orchestrator::agent_evidence) execution_liveness: String,
	pub(in crate::orchestrator::agent_evidence) ownership_state: String,
	pub(in crate::orchestrator::agent_evidence) liveness_state: String,
	pub(in crate::orchestrator::agent_evidence) policy_state: String,
	pub(in crate::orchestrator::agent_evidence) terminalization_state: String,
	pub(in crate::orchestrator::agent_evidence) lane_control_next_action: String,
	pub(in crate::orchestrator::agent_evidence) lane_control_conditions: Vec<String>,
	pub(in crate::orchestrator::agent_evidence) run_lease: bool,
	pub(in crate::orchestrator::agent_evidence) continuation_pending: bool,
	pub(in crate::orchestrator::agent_evidence) suspected_stall: bool,
	pub(in crate::orchestrator::agent_evidence) thread_id: Option<String>,
	pub(in crate::orchestrator::agent_evidence) turn_id: Option<String>,
	pub(in crate::orchestrator::agent_evidence) thread_status: Option<String>,
	pub(in crate::orchestrator::agent_evidence) thread_active_flags: Vec<String>,
	pub(in crate::orchestrator::agent_evidence) interactive_requested: bool,
	pub(in crate::orchestrator::agent_evidence) process_id: Option<u32>,
	pub(in crate::orchestrator::agent_evidence) process_alive: Option<bool>,
	pub(in crate::orchestrator::agent_evidence) process_liveness_reason: Option<String>,
	pub(in crate::orchestrator::agent_evidence) event_count: i64,
	pub(in crate::orchestrator::agent_evidence) last_event_type: Option<String>,
	pub(in crate::orchestrator::agent_evidence) last_event_at: Option<String>,
	pub(in crate::orchestrator::agent_evidence) last_run_activity_at: Option<String>,
	pub(in crate::orchestrator::agent_evidence) last_protocol_activity_at: Option<String>,
	pub(in crate::orchestrator::agent_evidence) last_progress_at: Option<String>,
	pub(in crate::orchestrator::agent_evidence) idle_for_seconds: Option<i64>,
	pub(in crate::orchestrator::agent_evidence) protocol_idle_for_seconds: Option<i64>,
	pub(in crate::orchestrator::agent_evidence) retry_kind: Option<String>,
	pub(in crate::orchestrator::agent_evidence) next_retry_at: Option<String>,
	pub(in crate::orchestrator::agent_evidence) effective_model: Option<String>,
	pub(in crate::orchestrator::agent_evidence) effective_model_provider: Option<String>,
	pub(in crate::orchestrator::agent_evidence) effective_cwd: Option<String>,
	pub(in crate::orchestrator::agent_evidence) effective_approval_policy: Option<String>,
	pub(in crate::orchestrator::agent_evidence) effective_approvals_reviewer: Option<String>,
	pub(in crate::orchestrator::agent_evidence) effective_sandbox_mode: Option<String>,
	pub(in crate::orchestrator::agent_evidence) branch_name: Option<String>,
	pub(in crate::orchestrator::agent_evidence) worktree_path: Option<String>,
	pub(in crate::orchestrator::agent_evidence) private_evidence: AgentPrivateEvidenceRef,
	pub(in crate::orchestrator::agent_evidence) ledger_outcome: Option<AgentRunLedgerOutcome>,
	pub(in crate::orchestrator::agent_evidence) diagnosis: AgentRunDiagnosis,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator::agent_evidence) struct AgentRunLedgerOutcome {
	pub(in crate::orchestrator::agent_evidence) ledger_status: String,
	pub(in crate::orchestrator::agent_evidence) final_outcome: String,
	pub(in crate::orchestrator::agent_evidence) final_event_type: Option<String>,
	pub(in crate::orchestrator::agent_evidence) final_event_at: Option<String>,
	pub(in crate::orchestrator::agent_evidence) summary: Option<String>,
	pub(in crate::orchestrator::agent_evidence) pr_url: Option<String>,
	pub(in crate::orchestrator::agent_evidence) commit_sha: Option<String>,
	pub(in crate::orchestrator::agent_evidence) closeout_status: Option<String>,
	pub(in crate::orchestrator::agent_evidence) needs_attention_reason: Option<String>,
	pub(in crate::orchestrator::agent_evidence) record_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator::agent_evidence) struct AgentRunDiagnosis {
	pub(in crate::orchestrator::agent_evidence) attention_required: bool,
	pub(in crate::orchestrator::agent_evidence) reason_code: Option<String>,
	pub(in crate::orchestrator::agent_evidence) next_action: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator::agent_evidence) struct AgentRecoveryWorktree {
	pub(in crate::orchestrator::agent_evidence) issue_id: String,
	pub(in crate::orchestrator::agent_evidence) issue_identifier: Option<String>,
	pub(in crate::orchestrator::agent_evidence) issue_state: Option<String>,
	pub(in crate::orchestrator::agent_evidence) branch_name: String,
	pub(in crate::orchestrator::agent_evidence) worktree_path: String,
	pub(in crate::orchestrator::agent_evidence) role: String,
	pub(in crate::orchestrator::agent_evidence) ownership: String,
	pub(in crate::orchestrator::agent_evidence) ownership_reason: String,
	pub(in crate::orchestrator::agent_evidence) hygiene_classification: Option<String>,
	pub(in crate::orchestrator::agent_evidence) hygiene_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator::agent_evidence) struct AgentRecoveryContract {
	pub(in crate::orchestrator::agent_evidence) evidence_ref: String,
	pub(in crate::orchestrator::agent_evidence) kind: String,
	pub(in crate::orchestrator::agent_evidence) issue_identifier: Option<String>,
	pub(in crate::orchestrator::agent_evidence) reason_code: String,
	pub(in crate::orchestrator::agent_evidence) command: Option<String>,
	pub(in crate::orchestrator::agent_evidence) next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator::agent_evidence) struct AgentEvidenceEvent {
	pub(in crate::orchestrator::agent_evidence) schema: &'static str,
	pub(in crate::orchestrator::agent_evidence) project_id: String,
	pub(in crate::orchestrator::agent_evidence) generated_at: String,
	pub(in crate::orchestrator::agent_evidence) source: String,
	pub(in crate::orchestrator::agent_evidence) handoff_index_path: String,
	pub(in crate::orchestrator::agent_evidence) blocker_count: usize,
	pub(in crate::orchestrator::agent_evidence) run_capsule_count: usize,
	pub(in crate::orchestrator::agent_evidence) warning_count: usize,
	pub(in crate::orchestrator::agent_evidence) connector_backoff_count: usize,
}
