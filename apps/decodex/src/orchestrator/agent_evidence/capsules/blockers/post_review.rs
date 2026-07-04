use crate::orchestrator::OperatorPostReviewLaneStatus;

pub(crate) fn post_review_lane_requires_attention(lane: &OperatorPostReviewLaneStatus) -> bool {
	matches!(
		lane.classification.as_str(),
		"blocked" | "needs_review_repair" | "closeout_blocked" | "cleanup_blocked"
	) || lane.reason == "missing_review_handoff_record"
}

pub(crate) fn post_review_lane_next_action(
	lane: &OperatorPostReviewLaneStatus,
	project_id: &str,
) -> String {
	if lane.reason == "missing_review_handoff_record" {
		return format!(
			"Run `decodex recover review-handoff diagnose {} --json`; rebind only after PR lineage and retained worktree HEAD match.",
			lane.issue_identifier
		);
	}
	if lane.classification == "needs_review_repair" {
		return String::from(
			"Run or inspect the retained review-repair lane before attempting land.",
		);
	}

	format!(
		"Inspect the `{}` retained post-review lane for service `{project_id}` before retrying.",
		lane.classification
	)
}
