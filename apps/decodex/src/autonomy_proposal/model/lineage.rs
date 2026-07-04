use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyProposalObjectiveLineage {
	pub(in crate::autonomy_proposal) project_id: String,
	pub(in crate::autonomy_proposal) objective_id: String,
	pub(in crate::autonomy_proposal) objective_version: u64,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::autonomy_proposal) objective_state: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::autonomy_proposal) objective_summary: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyProposalSourceSignal {
	pub(in crate::autonomy_proposal) signal_id: String,
	pub(in crate::autonomy_proposal) kind: String,
	pub(in crate::autonomy_proposal) freshness: String,
	pub(in crate::autonomy_proposal) evidence_class: String,
	pub(in crate::autonomy_proposal) confidence: String,
	#[serde(default)]
	pub(in crate::autonomy_proposal) gaps: Vec<String>,
	#[serde(default)]
	pub(in crate::autonomy_proposal) contradictions: Vec<String>,
}
