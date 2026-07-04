use crate::agent::tracker_tool_bridge::{
	NormalizedRejectedReviewCheckpointFinding, NormalizedReviewCheckpointFinding,
	NormalizedReviewCheckpointFindingRoute,
	tools::review_checkpoint::{
		REVIEW_ROUTE_CURRENT_BLOCKER, REVIEW_ROUTE_REVIEWER_RUBRIC_GAP,
		REVIEW_ROUTE_SOURCE_ACCEPTED, REVIEW_ROUTE_SOURCE_REJECTED,
	},
};

pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint::routes) fn default_current_blocker_route(
	index: u64,
	finding: &NormalizedReviewCheckpointFinding,
) -> NormalizedReviewCheckpointFindingRoute {
	NormalizedReviewCheckpointFindingRoute {
		route: String::from(REVIEW_ROUTE_CURRENT_BLOCKER),
		severity: finding.severity.clone(),
		risk_tier: String::from("medium"),
		summary: finding.summary.clone(),
		evidence: finding.evidence.clone(),
		resolver: String::from("agent"),
		next_action: finding.guidance.clone(),
		finding_source: String::from(REVIEW_ROUTE_SOURCE_ACCEPTED),
		finding_index: Some(index),
		finding_fingerprint: Some(finding.fingerprint.clone()),
	}
}

pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint::routes) fn default_reviewer_rubric_gap_route(
	index: u64,
	finding: &NormalizedRejectedReviewCheckpointFinding,
) -> NormalizedReviewCheckpointFindingRoute {
	NormalizedReviewCheckpointFindingRoute {
		route: String::from(REVIEW_ROUTE_REVIEWER_RUBRIC_GAP),
		severity: finding.severity.clone(),
		risk_tier: String::from("low"),
		summary: finding.summary.clone(),
		evidence: finding.evidence.clone(),
		resolver: String::from("reviewer"),
		next_action: finding.rejection_reason.clone(),
		finding_source: String::from(REVIEW_ROUTE_SOURCE_REJECTED),
		finding_index: Some(index),
		finding_fingerprint: None,
	}
}
