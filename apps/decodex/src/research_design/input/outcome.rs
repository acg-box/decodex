use serde::{Deserialize, Serialize};

use crate::loop_contract::DecisionContractStatus;

/// Research/design outcome before any execution authority exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResearchDesignOutcome {
	DecisionReady,
	NotDecisionReady,
	Blocked,
	NeedsHumanDecision,
}
impl ResearchDesignOutcome {
	pub(in crate::research_design) fn contract_status(self) -> DecisionContractStatus {
		match self {
			Self::DecisionReady | Self::NotDecisionReady | Self::Blocked =>
				DecisionContractStatus::DraftLatent,
			Self::NeedsHumanDecision => DecisionContractStatus::NeedsHumanDecision,
		}
	}
}
