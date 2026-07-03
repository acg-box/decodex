mod objective;
mod proposal;
mod signal;

pub(in crate::mcp) use self::{
	objective::{AutonomyAcceptObjectiveToolArgs, AutonomyDraftObjectiveToolArgs},
	proposal::{
		AutonomyChallengeProposalToolArgs, AutonomyCompileProposalToolArgs,
		AutonomyRequestPromotionToolArgs,
	},
	signal::{AutonomySignalInputArgs, AutonomySubmitSignalToolArgs},
};
