use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomyProposalState {
	Draft,
	NeedsEvidence,
	NeedsHumanDecision,
	Rejected,
	DecisionCandidate,
	AcceptedPromoted,
}
