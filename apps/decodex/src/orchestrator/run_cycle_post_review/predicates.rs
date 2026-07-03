use crate::orchestrator::{OperatorPostReviewLaneStatus, PostReviewLaneDecision};

pub(crate) fn post_review_lane_is_closeout_candidate(
	lane: &OperatorPostReviewLaneStatus,
	_completed_state: &str,
) -> bool {
	PostReviewLaneDecision::from_str(&lane.classification) == Some(PostReviewLaneDecision::Continue)
		&& lane.reason == "pull_request_merged_closeout_pending"
}

pub(crate) fn post_review_lane_is_repair_candidate(lane: &OperatorPostReviewLaneStatus) -> bool {
	PostReviewLaneDecision::from_str(&lane.classification)
		== Some(PostReviewLaneDecision::NeedsReviewRepair)
}
