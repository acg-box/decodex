//! Versioned dry-run autonomy proposal evidence.

mod authority;
mod decision;
mod evidence;
mod model;
mod proposal;
mod validation;

pub(crate) use self::model::{
	AutonomyProposal, AutonomyProposalAcceptedProjectPolicy, AutonomyProposalAuthorityActorKind,
	AutonomyProposalChallengeEvidence, AutonomyProposalChallengeInput,
	AutonomyProposalChallengeSource, AutonomyProposalCompileInput,
	AutonomyProposalDecisionBridgeAuthority, AutonomyProposalDecisionBridgeAuthorityInput,
	AutonomyProposalIssueCandidate, AutonomyProposalObjectiveLineage, AutonomyProposalRefusal,
	AutonomyProposalRefusalReason, AutonomyProposalSourceSignal, AutonomyProposalState,
};

use crate::prelude::{Result, eyre};

pub(crate) const AUTONOMY_PROPOSAL_SCHEMA: &str = "decodex.autonomy_proposal/1";

const AUTONOMY_PROPOSAL_RECORD_VERSION: u16 = 1;
const AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE: &str = "autonomy_proposal_acceptance";

#[cfg(test)] mod tests;
