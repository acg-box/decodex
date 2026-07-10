mod objective;
mod proposal;
mod runtime_policy;
mod signal;

pub(in crate::mcp) use self::{
	objective::{AutonomyAcceptObjectiveToolArgs, AutonomyDraftObjectiveToolArgs},
	proposal::{
		AutonomyChallengeProposalToolArgs, AutonomyCompileProposalToolArgs,
		AutonomyRequestPromotionToolArgs,
	},
	runtime_policy::{AutonomyAcceptRuntimePolicyToolArgs, AutonomyApplyRuntimePolicyToolArgs},
	signal::{AutonomySignalInputArgs, AutonomySubmitSignalToolArgs},
};
