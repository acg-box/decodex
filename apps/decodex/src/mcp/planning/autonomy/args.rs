mod objective;
mod proposal;
mod signal;

pub(super) use self::{
	objective::{AutonomyAcceptObjectiveToolArgs, AutonomyDraftObjectiveToolArgs},
	proposal::{
		AutonomyChallengeProposalToolArgs, AutonomyCompileProposalToolArgs,
		AutonomyRequestPromotionToolArgs,
	},
	signal::{AutonomySignalInputArgs, AutonomySubmitSignalToolArgs},
};
