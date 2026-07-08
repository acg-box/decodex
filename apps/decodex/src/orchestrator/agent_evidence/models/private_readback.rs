use serde::Serialize;
use serde_json::Value;

use crate::orchestrator::harness_improvement::HarnessImprovementCandidateSummary;

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
	pub(crate) validation_evidence: Vec<PrivateEvidenceValidationSummary>,
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
pub(crate) struct PrivateEvidenceValidationSummary {
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
