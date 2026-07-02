//! Versioned Loop/Decision Contract model for research-to-execution handoff.

pub(crate) mod authority;
pub(crate) mod evidence;
pub(crate) mod links;
pub(crate) mod promotion;
pub(crate) mod readiness;
pub(crate) mod research;
pub(crate) mod schema;
pub(crate) mod source_intent;
pub(crate) mod validation;

mod contract;
mod transitions;

pub(crate) use self::{
	authority::DecisionAcceptedAuthority,
	contract::DecisionContract,
	evidence::DecisionEvidenceBoundary,
	links::DecisionContractLinks,
	promotion::DecisionPromotion,
	readiness::{DecisionExecutionReadiness, DecisionProposedIssue},
	research::{DecisionResearchEvidence, DecisionResearchOption, DecisionResearchProvenance},
	schema::{
		DECISION_CONTRACT_RECORD_VERSION, DECISION_CONTRACT_SCHEMA, DecisionContractStatus,
		DecisionPromotionActorKind,
	},
	source_intent::DecisionSourceIntent,
};

#[cfg(test)] mod tests;
