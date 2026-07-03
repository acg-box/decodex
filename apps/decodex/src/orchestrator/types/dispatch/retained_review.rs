use crate::orchestrator::types::{
	PostReviewLaneClassification, PullRequestReviewState, RetainedReviewLane,
	RetainedReviewLaneBlocked,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PostReviewLaneDecision {
	Continue,
	WaitForReview,
	NeedsReviewRepair,
	ReadyToLand,
	CloseoutBlocked,
	CleanupBlocked,
	Block,
}
impl PostReviewLaneDecision {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Continue => "continue",
			Self::WaitForReview => "wait_for_review",
			Self::NeedsReviewRepair => "needs_review_repair",
			Self::ReadyToLand => "ready_to_land",
			Self::CloseoutBlocked => "closeout_blocked",
			Self::CleanupBlocked => "cleanup_blocked",
			Self::Block => "blocked",
		}
	}

	pub(crate) fn from_str(value: &str) -> Option<Self> {
		Some(match value {
			"continue" => Self::Continue,
			"wait_for_review" => Self::WaitForReview,
			"needs_review_repair" => Self::NeedsReviewRepair,
			"ready_to_land" => Self::ReadyToLand,
			"closeout_blocked" => Self::CloseoutBlocked,
			"cleanup_blocked" => Self::CleanupBlocked,
			"blocked" => Self::Block,
			_ => return None,
		})
	}
}

pub(crate) enum RetainedReviewLaneLoad {
	Skip,
	Wait(String),
	Ready(Box<RetainedReviewLane>),
	Blocked(Box<RetainedReviewLaneBlocked>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReviewOrchestrationPhase {
	RequestPending,
	WaitingForAck,
	WaitingForResult,
	RepairRequired,
	PassWaitingForGates,
	WaitingForMerge,
}
impl ReviewOrchestrationPhase {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::RequestPending => "request_pending",
			Self::WaitingForAck => "waiting_for_ack",
			Self::WaitingForResult => "waiting_for_result",
			Self::RepairRequired => "repair_required",
			Self::PassWaitingForGates => "pass_waiting_for_gates",
			Self::WaitingForMerge => "waiting_for_merge",
		}
	}

	pub(crate) fn parse(value: &str) -> std::result::Result<Self, String> {
		match value {
			"request_pending" => Ok(Self::RequestPending),
			"waiting_for_ack" => Ok(Self::WaitingForAck),
			"waiting_for_result" => Ok(Self::WaitingForResult),
			"repair_required" => Ok(Self::RepairRequired),
			"pass_waiting_for_gates" => Ok(Self::PassWaitingForGates),
			"waiting_for_merge" => Ok(Self::WaitingForMerge),
			other => Err(format!(
				"Unknown review orchestration phase `{other}` in retained review marker."
			)),
		}
	}
}

pub(crate) enum PostReviewLaneStateLoad {
	Classification(PostReviewLaneClassification),
	ReviewState(PullRequestReviewState),
}
