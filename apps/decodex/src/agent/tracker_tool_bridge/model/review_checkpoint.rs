use serde::{Deserialize, Serialize};

use super::args::ScopeArgs;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::agent::tracker_tool_bridge) struct ReviewCheckpointArgs {
	#[serde(flatten)]
	pub(in crate::agent::tracker_tool_bridge) scope: ScopeArgs,
	pub(in crate::agent::tracker_tool_bridge) reviewer: Option<String>,
	pub(in crate::agent::tracker_tool_bridge) status: String,
	pub(in crate::agent::tracker_tool_bridge) head_sha: String,
	pub(in crate::agent::tracker_tool_bridge) review_contract: Option<ReviewCheckpointContractArgs>,
	pub(in crate::agent::tracker_tool_bridge) review_cost_control: Option<ReviewCostControlArgs>,
	pub(in crate::agent::tracker_tool_bridge) checks: Option<ReviewCheckpointChecksArgs>,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) evidence: Vec<String>,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) accepted_findings: Vec<ReviewCheckpointFindingArgs>,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) rejected_findings:
		Vec<ReviewCheckpointRejectedFindingArgs>,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) finding_routes: Vec<ReviewCheckpointFindingRouteArgs>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::agent::tracker_tool_bridge) struct ReviewCostControlArgs {
	pub(in crate::agent::tracker_tool_bridge) review_class: String,
	pub(in crate::agent::tracker_tool_bridge) risk_class: String,
	pub(in crate::agent::tracker_tool_bridge) changed_surface_count: u64,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) changed_surface_summary: Vec<String>,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) high_risk_surfaces: Vec<String>,
	pub(in crate::agent::tracker_tool_bridge) current_head_evidence: bool,
	pub(in crate::agent::tracker_tool_bridge) validation_backed: bool,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) validation_current: bool,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) evidence_sufficient: bool,
	pub(in crate::agent::tracker_tool_bridge) reviewer_judgment: String,
	pub(in crate::agent::tracker_tool_bridge) fallback_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::agent::tracker_tool_bridge) struct ReviewCheckpointContractArgs {
	pub(in crate::agent::tracker_tool_bridge) workflow_policy_source: String,
	pub(in crate::agent::tracker_tool_bridge) review_type: String,
	pub(in crate::agent::tracker_tool_bridge) risk_tier: String,
	pub(in crate::agent::tracker_tool_bridge) objective: String,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) scope: Vec<String>,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) non_goals: Vec<String>,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) required_checks: Vec<String>,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) allowed_expansion_triggers: Vec<String>,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) validation_evidence: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::agent::tracker_tool_bridge) struct ReviewCheckpointChecksArgs {
	pub(in crate::agent::tracker_tool_bridge) intended_behavior: String,
	pub(in crate::agent::tracker_tool_bridge) regression_risk: String,
	pub(in crate::agent::tracker_tool_bridge) missing_tests: String,
	pub(in crate::agent::tracker_tool_bridge) docs_config_drift: String,
	pub(in crate::agent::tracker_tool_bridge) migration_fallout: String,
	pub(in crate::agent::tracker_tool_bridge) operator_facing_fallout: String,
	pub(in crate::agent::tracker_tool_bridge) loop_decision_contract: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::agent::tracker_tool_bridge) struct ReviewCheckpointFindingArgs {
	pub(in crate::agent::tracker_tool_bridge) severity: String,
	pub(in crate::agent::tracker_tool_bridge) summary: String,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) evidence: Vec<String>,
	pub(in crate::agent::tracker_tool_bridge) kind: Option<String>,
	pub(in crate::agent::tracker_tool_bridge) file: Option<String>,
	pub(in crate::agent::tracker_tool_bridge) line: Option<u64>,
	pub(in crate::agent::tracker_tool_bridge) line_range: Option<ReviewCheckpointLineRangeArgs>,
	pub(in crate::agent::tracker_tool_bridge) guidance: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::agent::tracker_tool_bridge) struct ReviewCheckpointRejectedFindingArgs {
	pub(in crate::agent::tracker_tool_bridge) severity: String,
	pub(in crate::agent::tracker_tool_bridge) summary: String,
	pub(in crate::agent::tracker_tool_bridge) rejection_reason: String,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) evidence: Vec<String>,
	pub(in crate::agent::tracker_tool_bridge) kind: Option<String>,
	pub(in crate::agent::tracker_tool_bridge) file: Option<String>,
	pub(in crate::agent::tracker_tool_bridge) line: Option<u64>,
	pub(in crate::agent::tracker_tool_bridge) line_range: Option<ReviewCheckpointLineRangeArgs>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::agent::tracker_tool_bridge) struct ReviewCheckpointFindingRouteArgs {
	pub(in crate::agent::tracker_tool_bridge) route: String,
	pub(in crate::agent::tracker_tool_bridge) severity: String,
	pub(in crate::agent::tracker_tool_bridge) summary: String,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) evidence: Vec<String>,
	pub(in crate::agent::tracker_tool_bridge) resolver: String,
	pub(in crate::agent::tracker_tool_bridge) next_action: String,
	pub(in crate::agent::tracker_tool_bridge) risk_tier: Option<String>,
	pub(in crate::agent::tracker_tool_bridge) finding_source: Option<String>,
	pub(in crate::agent::tracker_tool_bridge) finding_index: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::agent::tracker_tool_bridge) struct ReviewCheckpointLineRangeArgs {
	pub(in crate::agent::tracker_tool_bridge) start: u64,
	pub(in crate::agent::tracker_tool_bridge) end: u64,
}

#[derive(Debug, Serialize)]
pub(in crate::agent::tracker_tool_bridge) struct NormalizedReviewCheckpointPayload {
	pub(in crate::agent::tracker_tool_bridge) reviewer: String,
	pub(in crate::agent::tracker_tool_bridge) review_contract: NormalizedReviewCheckpointContract,
	pub(in crate::agent::tracker_tool_bridge) review_contract_hash: String,
	pub(in crate::agent::tracker_tool_bridge) review_cost_control: NormalizedReviewCostControl,
	pub(in crate::agent::tracker_tool_bridge) reviewed_head: ReviewCheckpointHeadBinding,
	pub(in crate::agent::tracker_tool_bridge) checks: ReviewCheckpointChecksArgs,
	pub(in crate::agent::tracker_tool_bridge) evidence: Vec<String>,
	pub(in crate::agent::tracker_tool_bridge) accepted_findings:
		Vec<NormalizedReviewCheckpointFinding>,
	pub(in crate::agent::tracker_tool_bridge) rejected_findings:
		Vec<NormalizedRejectedReviewCheckpointFinding>,
	pub(in crate::agent::tracker_tool_bridge) finding_routes:
		Vec<NormalizedReviewCheckpointFindingRoute>,
	pub(in crate::agent::tracker_tool_bridge) finding_route_summary:
		ReviewCheckpointFindingRouteSummary,
	pub(in crate::agent::tracker_tool_bridge) finding_policy: ReviewFindingPolicyState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::agent::tracker_tool_bridge) struct NormalizedReviewCheckpointContract {
	pub(in crate::agent::tracker_tool_bridge) workflow_policy_source: String,
	pub(in crate::agent::tracker_tool_bridge) review_type: String,
	pub(in crate::agent::tracker_tool_bridge) risk_tier: String,
	pub(in crate::agent::tracker_tool_bridge) objective: String,
	pub(in crate::agent::tracker_tool_bridge) scope: Vec<String>,
	pub(in crate::agent::tracker_tool_bridge) non_goals: Vec<String>,
	pub(in crate::agent::tracker_tool_bridge) required_checks: Vec<String>,
	pub(in crate::agent::tracker_tool_bridge) allowed_expansion_triggers: Vec<String>,
	pub(in crate::agent::tracker_tool_bridge) validation_evidence: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::agent::tracker_tool_bridge) struct NormalizedReviewCostControl {
	pub(in crate::agent::tracker_tool_bridge) review_class: String,
	pub(in crate::agent::tracker_tool_bridge) risk_class: String,
	pub(in crate::agent::tracker_tool_bridge) compact_eligible: bool,
	pub(in crate::agent::tracker_tool_bridge) changed_surface_count: u64,
	pub(in crate::agent::tracker_tool_bridge) changed_surface_summary: Vec<String>,
	pub(in crate::agent::tracker_tool_bridge) high_risk_surfaces: Vec<String>,
	pub(in crate::agent::tracker_tool_bridge) current_head_evidence: bool,
	pub(in crate::agent::tracker_tool_bridge) validation_backed: bool,
	pub(in crate::agent::tracker_tool_bridge) validation_current: bool,
	pub(in crate::agent::tracker_tool_bridge) evidence_sufficient: bool,
	pub(in crate::agent::tracker_tool_bridge) reviewer_judgment: String,
	pub(in crate::agent::tracker_tool_bridge) fallback_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::agent::tracker_tool_bridge) struct ReviewCheckpointHeadBinding {
	pub(in crate::agent::tracker_tool_bridge) head_sha: String,
	pub(in crate::agent::tracker_tool_bridge) head_tree_oid: String,
	pub(in crate::agent::tracker_tool_bridge) review_worktree_clean: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::agent::tracker_tool_bridge) struct NormalizedReviewCheckpointFinding {
	pub(in crate::agent::tracker_tool_bridge) severity: String,
	pub(in crate::agent::tracker_tool_bridge) summary: String,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) evidence: Vec<String>,
	pub(in crate::agent::tracker_tool_bridge) kind: String,
	pub(in crate::agent::tracker_tool_bridge) file: Option<String>,
	pub(in crate::agent::tracker_tool_bridge) line: Option<u64>,
	pub(in crate::agent::tracker_tool_bridge) line_range: Option<ReviewCheckpointLineRangeArgs>,
	pub(in crate::agent::tracker_tool_bridge) guidance: String,
	pub(in crate::agent::tracker_tool_bridge) fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::agent::tracker_tool_bridge) struct NormalizedRejectedReviewCheckpointFinding {
	pub(in crate::agent::tracker_tool_bridge) severity: String,
	pub(in crate::agent::tracker_tool_bridge) summary: String,
	pub(in crate::agent::tracker_tool_bridge) rejection_reason: String,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) evidence: Vec<String>,
	pub(in crate::agent::tracker_tool_bridge) kind: String,
	pub(in crate::agent::tracker_tool_bridge) file: Option<String>,
	pub(in crate::agent::tracker_tool_bridge) line: Option<u64>,
	pub(in crate::agent::tracker_tool_bridge) line_range: Option<ReviewCheckpointLineRangeArgs>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::agent::tracker_tool_bridge) struct NormalizedReviewCheckpointFindingRoute {
	pub(in crate::agent::tracker_tool_bridge) route: String,
	pub(in crate::agent::tracker_tool_bridge) severity: String,
	pub(in crate::agent::tracker_tool_bridge) risk_tier: String,
	pub(in crate::agent::tracker_tool_bridge) summary: String,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) evidence: Vec<String>,
	pub(in crate::agent::tracker_tool_bridge) resolver: String,
	pub(in crate::agent::tracker_tool_bridge) next_action: String,
	pub(in crate::agent::tracker_tool_bridge) finding_source: String,
	pub(in crate::agent::tracker_tool_bridge) finding_index: Option<u64>,
	pub(in crate::agent::tracker_tool_bridge) finding_fingerprint: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::agent::tracker_tool_bridge) struct ReviewCheckpointFindingRouteSummary {
	pub(in crate::agent::tracker_tool_bridge) route_counts: Vec<ReviewCheckpointFindingRouteCount>,
	pub(in crate::agent::tracker_tool_bridge) next_action: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::agent::tracker_tool_bridge) struct ReviewCheckpointFindingRouteCount {
	pub(in crate::agent::tracker_tool_bridge) route: String,
	pub(in crate::agent::tracker_tool_bridge) count: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub(in crate::agent::tracker_tool_bridge) struct ReviewFindingPolicyState {
	pub(in crate::agent::tracker_tool_bridge) schema: String,
	pub(in crate::agent::tracker_tool_bridge) phase: String,
	pub(in crate::agent::tracker_tool_bridge) status: String,
	pub(in crate::agent::tracker_tool_bridge) head_sha: String,
	pub(in crate::agent::tracker_tool_bridge) nonclean_rounds: i64,
	pub(in crate::agent::tracker_tool_bridge) active_fingerprints: Vec<String>,
	pub(in crate::agent::tracker_tool_bridge) stop_fingerprint: Option<String>,
	pub(in crate::agent::tracker_tool_bridge) findings: Vec<ReviewFindingPolicyRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(in crate::agent::tracker_tool_bridge) struct ReviewFindingPolicyRecord {
	pub(in crate::agent::tracker_tool_bridge) fingerprint: String,
	pub(in crate::agent::tracker_tool_bridge) kind: String,
	pub(in crate::agent::tracker_tool_bridge) title: String,
	pub(in crate::agent::tracker_tool_bridge) body: String,
	pub(in crate::agent::tracker_tool_bridge) file: Option<String>,
	pub(in crate::agent::tracker_tool_bridge) line_range: Option<ReviewCheckpointLineRangeArgs>,
	pub(in crate::agent::tracker_tool_bridge) first_seen_head: String,
	pub(in crate::agent::tracker_tool_bridge) last_seen_head: String,
	pub(in crate::agent::tracker_tool_bridge) status: String,
	pub(in crate::agent::tracker_tool_bridge) repeat_count: i64,
	pub(in crate::agent::tracker_tool_bridge) repair_evidence: Vec<String>,
}
