use serde::{Deserialize, Serialize};

use crate::orchestrator::types::operator_status::queue::OperatorAuthorityDecisionRequestStatus;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorAutonomyProposalRefusalStatus {
	pub(crate) reason: String,
	pub(crate) detail: String,
	pub(crate) evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorAutonomyDecisionContractStatus {
	pub(crate) contract_id: String,
	pub(crate) status: String,
	pub(crate) updated_at: String,
	pub(crate) generated_issue_identifiers: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorAutonomyProgramIntakeStatus {
	pub(crate) program_id: String,
	pub(crate) plan_id: String,
	pub(crate) intake_kind: String,
	pub(crate) source_contract_id: String,
	pub(crate) public_summary: String,
	pub(crate) updated_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorAutonomyExecutionEvidenceStatus {
	pub(crate) kind: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) source_refs: Vec<String>,
	pub(crate) summary: String,
	pub(crate) updated_at: String,
	pub(crate) completeness: String,
	pub(crate) known_gaps: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorAutonomyReportReadbackStatus {
	pub(crate) surface: String,
	pub(crate) authority: String,
	pub(crate) audit_authority: bool,
	pub(crate) source_refs: Vec<String>,
	pub(crate) redaction_level: String,
	pub(crate) completeness: String,
	pub(crate) known_gaps: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorReviewLoopStatus {
	pub(crate) phase: String,
	pub(crate) status: String,
	pub(crate) checkpoint: Option<OperatorReviewCheckpointStatus>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorReviewRouteCount {
	pub(crate) route: String,
	pub(crate) count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorRecoveryBudgetStatus {
	pub(crate) attempt: u64,
	pub(crate) max_attempts: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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
