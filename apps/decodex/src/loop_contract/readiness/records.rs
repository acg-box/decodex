use serde::{Deserialize, Serialize};

/// Natural-language readiness summary for later issue shaping.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionExecutionReadiness {
	pub(in crate::loop_contract) summary: String,
	pub(in crate::loop_contract) ready_for_issue_shaping: bool,
	#[serde(default)]
	pub(in crate::loop_contract) missing_decisions: Vec<String>,
	#[serde(default)]
	pub(in crate::loop_contract) validation_expectations: Vec<String>,
	#[serde(default)]
	pub(in crate::loop_contract) risk_notes: Vec<String>,
	pub(in crate::loop_contract) proposed_issues: Vec<DecisionProposedIssue>,
	#[serde(default)]
	pub(in crate::loop_contract) promotion_targets: Vec<String>,
	#[serde(default)]
	pub(in crate::loop_contract) conflict_domains: Vec<String>,
}

/// Structured issue-shaping input retained inside Decision Contract readiness.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionProposedIssue {
	pub(in crate::loop_contract) key: String,
	pub(in crate::loop_contract) title: String,
	pub(in crate::loop_contract) objective: String,
	pub(in crate::loop_contract) stage: String,
	pub(in crate::loop_contract) dependencies: Vec<String>,
	pub(in crate::loop_contract) conflict_domains: Vec<String>,
	pub(in crate::loop_contract) acceptance: Vec<String>,
	pub(in crate::loop_contract) validation: Vec<String>,
	pub(in crate::loop_contract) risk: Vec<String>,
	pub(in crate::loop_contract) queue_intent: String,
}
