use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::orchestrator::{
	OperatorGitHubCliAuthority, harness_improvement::HarnessImprovementCandidateSummary,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentEvidenceSource {
	DiagnoseCommand,
	ServeTick,
}
impl AgentEvidenceSource {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::DiagnoseCommand => "diagnose_command",
			Self::ServeTick => "serve_tick",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentEvidenceWriteResult {
	pub(crate) project_id: String,
	pub(crate) handoff_index_path: String,
	pub(crate) handoff_index: AgentHandoffIndex,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentEvidenceSummary {
	pub(crate) project_count: usize,
	pub(crate) current_lane_count: usize,
	pub(crate) recent_run_count: usize,
	pub(crate) history_lane_count: usize,
	pub(crate) queued_candidate_count: usize,
	pub(crate) post_review_lane_count: usize,
	pub(crate) recovery_worktree_count: usize,
	pub(crate) blocker_count: usize,
	pub(crate) run_capsule_count: usize,
	pub(crate) connector_backoff_count: usize,
	pub(crate) warning_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct AgentPrivateEvidenceRef {
	pub(crate) evidence_ref: String,
	pub(crate) source: String,
	pub(crate) default_view: String,
	pub(crate) read_command: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct PrivateEvidenceReadback {
	pub(crate) schema: &'static str,
	pub(crate) project_id: String,
	pub(crate) issue_selector: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) source: &'static str,
	pub(crate) evidence_ref: String,
	pub(crate) read_command: String,
	pub(crate) payload_mode: &'static str,
	pub(crate) event_count: usize,
	pub(crate) latest_event_type: Option<String>,
	pub(crate) latest_event_at: Option<String>,
	pub(crate) review_checkpoints: Vec<PrivateEvidenceReviewCheckpointSummary>,
	pub(crate) repo_gate_failures: Vec<PrivateEvidenceRepoGateFailureSummary>,
	pub(crate) phase_acceptance_checks: Vec<PrivateEvidencePhaseAcceptanceSummary>,
	pub(crate) boundary_checks: Vec<PrivateEvidenceBoundaryCheckSummary>,
	pub(crate) decision_requests: Vec<PrivateEvidenceDecisionRequestSummary>,
	pub(crate) architecture_recoveries: Vec<PrivateEvidenceArchitectureRecoverySummary>,
	pub(crate) improvement_candidates: Vec<HarnessImprovementCandidateSummary>,
	pub(crate) events: Vec<PrivateEvidenceReadbackEvent>,
	pub(crate) warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct PrivateEvidenceDecisionRequestSummary {
	pub(crate) decision_request_id: String,
	pub(crate) phase: String,
	pub(crate) reason: String,
	pub(crate) boundary: String,
	pub(crate) next_action: String,
	pub(crate) recommendation: Option<String>,
	pub(crate) resume_condition: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct PrivateEvidenceReviewCheckpointSummary {
	pub(crate) phase: String,
	pub(crate) status: String,
	pub(crate) head_sha: Option<String>,
	pub(crate) round: Option<u64>,
	pub(crate) review_class: Option<String>,
	pub(crate) risk_class: Option<String>,
	pub(crate) compact_eligible: Option<bool>,
	pub(crate) fallback_reason: Option<String>,
	pub(crate) active_fingerprints: Vec<String>,
	pub(crate) stop_fingerprint: Option<String>,
	pub(crate) accepted_finding_count: usize,
	pub(crate) rejected_finding_count: usize,
	pub(crate) route_counts: Vec<PrivateEvidenceReviewRouteCount>,
	pub(crate) route_next_action: Option<String>,
	pub(crate) next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct PrivateEvidenceReviewRouteCount {
	pub(crate) route: String,
	pub(crate) count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct PrivateEvidenceRepoGateFailureSummary {
	pub(crate) record_id: i64,
	pub(crate) phase: String,
	pub(crate) error_class: String,
	pub(crate) disposition: String,
	pub(crate) stage: Option<String>,
	pub(crate) failed_command: Option<String>,
	pub(crate) exit_status: Option<i64>,
	pub(crate) summary: Option<String>,
	pub(crate) problem_lines: Vec<String>,
	pub(crate) output_excerpt: Option<String>,
	pub(crate) output_truncated: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct PrivateEvidencePhaseAcceptanceSummary {
	pub(crate) phase: String,
	pub(crate) decision: String,
	pub(crate) reason_code: String,
	pub(crate) objective_covered: bool,
	pub(crate) effective_delta_present: bool,
	pub(crate) changed_surfaces: Vec<String>,
	pub(crate) non_goal_passed: bool,
	pub(crate) validation_passed: bool,
	pub(crate) next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct PrivateEvidenceBoundaryCheckSummary {
	pub(crate) disposition: String,
	pub(crate) policy_decision: String,
	pub(crate) reason: Option<String>,
	pub(crate) attempted_recovery_reason: Option<String>,
	pub(crate) decision_contract_count: usize,
	pub(crate) changed_surface_count: usize,
	pub(crate) improvement_signal_count: usize,
	pub(crate) requires_enhanced_evidence: bool,
	pub(crate) blocks_landing: bool,
	pub(crate) next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct PrivateEvidenceArchitectureRecoverySummary {
	pub(crate) reason_code: String,
	pub(crate) guardrail_reason: Option<String>,
	pub(crate) boundary_disposition: Option<String>,
	pub(crate) boundary_policy_decision: Option<String>,
	pub(crate) requires_enhanced_evidence: bool,
	pub(crate) blocks_landing: bool,
	pub(crate) recovery_budget_attempt: Option<u64>,
	pub(crate) recovery_budget_max_attempts: Option<u64>,
	pub(crate) next_action: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct PrivateEvidenceReadbackEvent {
	pub(crate) record_id: i64,
	pub(crate) event_type: String,
	pub(crate) recorded_at: String,
	pub(crate) payload_summary: PrivateEvidencePayloadSummary,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) payload: Option<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct PrivateEvidencePayloadSummary {
	pub(crate) kind: String,
	pub(crate) byte_count: usize,
	pub(crate) keys: Vec<String>,
	pub(crate) preview: Vec<String>,
	pub(crate) redacted_default_keys: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentHandoffIndex {
	pub(crate) schema: &'static str,
	pub(crate) project_id: String,
	pub(crate) generated_at: String,
	pub(crate) source: String,
	pub(crate) evidence_root: String,
	pub(crate) handoff_index_path: String,
	pub(crate) blockers_dir: String,
	pub(crate) runs_dir: String,
	pub(crate) events_path: String,
	pub(crate) summary: AgentEvidenceSummary,
	pub(crate) github_cli_authority: Option<OperatorGitHubCliAuthority>,
	pub(crate) warnings: Vec<String>,
	pub(crate) connector_backoffs: Vec<AgentConnectorBackoff>,
	pub(crate) blockers: Vec<AgentBlocker>,
	pub(crate) run_capsules: Vec<AgentRunCapsuleRef>,
	pub(crate) recovery_worktrees: Vec<AgentRecoveryWorktree>,
	pub(crate) recovery_contracts: Vec<AgentRecoveryContract>,
}

pub(crate) struct PrivateEvidenceTarget {
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
}

pub(crate) struct AgentEvidenceFileWriteContext<'a> {
	pub(crate) project_id: &'a str,
	pub(crate) generated_at: &'a str,
	pub(crate) source: AgentEvidenceSource,
	pub(crate) handoff_index_path: &'a Path,
	pub(crate) blockers_dir: &'a Path,
	pub(crate) events_path: &'a Path,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentConnectorBackoff {
	pub(crate) evidence_ref: String,
	pub(crate) connector: String,
	pub(crate) sync_phase: String,
	pub(crate) quota_class: String,
	pub(crate) reset_at: String,
	pub(crate) reset_unix_epoch: i64,
	pub(crate) reset_source: String,
	pub(crate) retry_after_seconds: i64,
	pub(crate) warning: String,
	pub(crate) next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentBlocker {
	pub(crate) evidence_ref: String,
	pub(crate) project_id: String,
	pub(crate) surface: String,
	pub(crate) issue_id: Option<String>,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) run_id: Option<String>,
	pub(crate) attempt_number: Option<i64>,
	pub(crate) classification: String,
	pub(crate) reason_code: String,
	pub(crate) reason: String,
	pub(crate) next_action: String,
	pub(crate) blocker_snapshot_path: String,
	pub(crate) related_run_capsule_path: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentBlockerSnapshot {
	pub(crate) schema: &'static str,
	pub(crate) project_id: String,
	pub(crate) generated_at: String,
	pub(crate) issue_id: Option<String>,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) blockers: Vec<AgentBlocker>,
	pub(crate) related_run_capsules: Vec<AgentRunCapsuleRef>,
}

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentRecoveryWorktree {
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) issue_state: Option<String>,
	pub(crate) branch_name: String,
	pub(crate) worktree_path: String,
	pub(crate) role: String,
	pub(crate) ownership: String,
	pub(crate) ownership_reason: String,
	pub(crate) hygiene_classification: Option<String>,
	pub(crate) hygiene_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentRecoveryContract {
	pub(crate) evidence_ref: String,
	pub(crate) kind: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) reason_code: String,
	pub(crate) command: Option<String>,
	pub(crate) next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentEvidenceEvent {
	pub(crate) schema: &'static str,
	pub(crate) project_id: String,
	pub(crate) generated_at: String,
	pub(crate) source: String,
	pub(crate) handoff_index_path: String,
	pub(crate) blocker_count: usize,
	pub(crate) run_capsule_count: usize,
	pub(crate) warning_count: usize,
	pub(crate) connector_backoff_count: usize,
}
