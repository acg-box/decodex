use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomyProposalRefusalReason {
	MissingObjective,
	DisallowedSignalKind,
	DisallowedSurface,
	StaleEvidence,
	UnresolvedContradiction,
	WeakenedValidationReview,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyProposalRefusal {
	pub(in crate::autonomy_proposal) reason: AutonomyProposalRefusalReason,
	pub(in crate::autonomy_proposal) detail: String,
	#[serde(default)]
	pub(in crate::autonomy_proposal) evidence_refs: Vec<String>,
}
