use serde::{Deserialize, Serialize};

use crate::agent::tracker_tool_bridge::model::args::ScopeArgs;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReviewCheckpointArgs {
	#[serde(flatten)]
	pub(crate) scope: ScopeArgs,
	pub(crate) reviewer: Option<String>,
	pub(crate) status: String,
	pub(crate) head_sha: String,
	pub(crate) review_contract: Option<ReviewCheckpointContractArgs>,
	pub(crate) review_cost_control: Option<ReviewCostControlArgs>,
	pub(crate) checks: Option<ReviewCheckpointChecksArgs>,
	#[serde(default)]
	pub(crate) evidence: Vec<String>,
	#[serde(default)]
	pub(crate) accepted_findings: Vec<ReviewCheckpointFindingArgs>,
	#[serde(default)]
	pub(crate) rejected_findings: Vec<ReviewCheckpointRejectedFindingArgs>,
	#[serde(default)]
	pub(crate) finding_routes: Vec<ReviewCheckpointFindingRouteArgs>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReviewCostControlArgs {
	pub(crate) review_class: String,
	pub(crate) risk_class: String,
	pub(crate) changed_surface_count: u64,
	#[serde(default)]
	pub(crate) changed_surface_summary: Vec<String>,
	#[serde(default)]
	pub(crate) high_risk_surfaces: Vec<String>,
	pub(crate) current_head_evidence: bool,
	pub(crate) validation_backed: bool,
	#[serde(default)]
	pub(crate) validation_current: bool,
	#[serde(default)]
	pub(crate) evidence_sufficient: bool,
	pub(crate) reviewer_judgment: String,
	pub(crate) fallback_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReviewCheckpointContractArgs {
	pub(crate) workflow_policy_source: String,
	pub(crate) review_type: String,
	pub(crate) risk_tier: String,
	pub(crate) objective: String,
	#[serde(default)]
	pub(crate) scope: Vec<String>,
	#[serde(default)]
	pub(crate) non_goals: Vec<String>,
	#[serde(default)]
	pub(crate) required_checks: Vec<String>,
	#[serde(default)]
	pub(crate) allowed_expansion_triggers: Vec<String>,
	#[serde(default)]
	pub(crate) validation_evidence: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReviewCheckpointChecksArgs {
	pub(crate) intended_behavior: String,
	pub(crate) regression_risk: String,
	pub(crate) missing_tests: String,
	pub(crate) docs_config_drift: String,
	pub(crate) migration_fallout: String,
	pub(crate) operator_facing_fallout: String,
	pub(crate) loop_decision_contract: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReviewCheckpointFindingArgs {
	pub(crate) severity: String,
	pub(crate) summary: String,
	#[serde(default)]
	pub(crate) evidence: Vec<String>,
	pub(crate) kind: Option<String>,
	pub(crate) file: Option<String>,
	pub(crate) line: Option<u64>,
	pub(crate) line_range: Option<ReviewCheckpointLineRangeArgs>,
	pub(crate) guidance: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReviewCheckpointRejectedFindingArgs {
	pub(crate) severity: String,
	pub(crate) summary: String,
	pub(crate) rejection_reason: String,
	#[serde(default)]
	pub(crate) evidence: Vec<String>,
	pub(crate) kind: Option<String>,
	pub(crate) file: Option<String>,
	pub(crate) line: Option<u64>,
	pub(crate) line_range: Option<ReviewCheckpointLineRangeArgs>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReviewCheckpointFindingRouteArgs {
	pub(crate) route: String,
	pub(crate) severity: String,
	pub(crate) summary: String,
	#[serde(default)]
	pub(crate) evidence: Vec<String>,
	pub(crate) resolver: String,
	pub(crate) next_action: String,
	pub(crate) risk_tier: Option<String>,
	pub(crate) finding_source: Option<String>,
	pub(crate) finding_index: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReviewCheckpointLineRangeArgs {
	pub(crate) start: u64,
	pub(crate) end: u64,
}
