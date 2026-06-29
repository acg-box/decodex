mod challenge;
mod compile;
mod promotion;

pub(in crate::mcp::planning::autonomy) use self::{
	challenge::AutonomyChallengeProposalToolArgs, compile::AutonomyCompileProposalToolArgs,
	promotion::AutonomyRequestPromotionToolArgs,
};
