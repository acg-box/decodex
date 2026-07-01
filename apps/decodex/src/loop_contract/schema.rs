use serde::{Deserialize, Serialize};

pub(crate) const DECISION_CONTRACT_SCHEMA: &str = "decodex.decision_contract/1";
pub(crate) const DECISION_CONTRACT_RECORD_VERSION: u16 = 1;

/// Runtime-facing state for a Loop/Decision Contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DecisionContractStatus {
	DraftLatent,
	AcceptedPromoted,
	RejectedSuperseded,
	NeedsHumanDecision,
}
impl DecisionContractStatus {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::DraftLatent => "draft_latent",
			Self::AcceptedPromoted => "accepted_promoted",
			Self::RejectedSuperseded => "rejected_superseded",
			Self::NeedsHumanDecision => "needs_human_decision",
		}
	}
}

/// Actor class that accepted or promoted the contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DecisionPromotionActorKind {
	User,
	RuntimePolicy,
}

pub(super) fn decision_contract_schema() -> String {
	DECISION_CONTRACT_SCHEMA.to_owned()
}

pub(super) fn decision_contract_record_version() -> u16 {
	DECISION_CONTRACT_RECORD_VERSION
}
