mod challenge;
mod compile;
mod promotion;

pub(in crate::mcp) use self::{
	challenge::AutonomyChallengeProposalToolArgs, compile::AutonomyCompileProposalToolArgs,
	promotion::AutonomyRequestPromotionToolArgs,
};
