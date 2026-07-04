mod authority;
mod challenge;
mod input;
mod issue;
mod lineage;
mod record;
mod refusal;
mod state;

pub(crate) use self::{
	authority::{AutonomyProposalAcceptedProjectPolicy, AutonomyProposalAuthorityActorKind},
	challenge::{
		AutonomyProposalChallengeEvidence, AutonomyProposalChallengeInput,
		AutonomyProposalChallengeSource,
	},
	input::{
		AutonomyProposalCompileInput, AutonomyProposalDecisionBridgeAuthority,
		AutonomyProposalDecisionBridgeAuthorityInput,
	},
	issue::AutonomyProposalIssueCandidate,
	lineage::{AutonomyProposalObjectiveLineage, AutonomyProposalSourceSignal},
	record::AutonomyProposal,
	refusal::{AutonomyProposalRefusal, AutonomyProposalRefusalReason},
	state::AutonomyProposalState,
};
