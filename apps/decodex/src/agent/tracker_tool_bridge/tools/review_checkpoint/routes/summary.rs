use crate::agent::tracker_tool_bridge::{
	NormalizedReviewCheckpointFindingRoute,
	tools::review_checkpoint::{
		REVIEW_ROUTE_ARCHITECTURE_SIGNAL, REVIEW_ROUTE_CONTRACT_OR_AUTHORITY_DECISION_REQUIRED,
		REVIEW_ROUTE_CURRENT_BLOCKER, REVIEW_ROUTE_DETERMINISTIC_GATE_CANDIDATE,
		REVIEW_ROUTE_FOLLOW_UP, REVIEW_ROUTE_INVALID_OR_UNSUBSTANTIATED,
		REVIEW_ROUTE_ISSUE_CONTRACT_GAP, REVIEW_ROUTE_LANDING_BLOCKER, REVIEW_ROUTE_NEEDS_EVIDENCE,
		REVIEW_ROUTE_REVIEWER_RUBRIC_GAP, REVIEW_ROUTE_RISK_NOTE,
	},
};

pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint::routes) fn review_route_next_action(
	routes: &[NormalizedReviewCheckpointFindingRoute],
) -> Option<String> {
	routes
		.iter()
		.min_by_key(|route| review_route_priority(&route.route))
		.map(|route| route.next_action.clone())
}

fn review_route_priority(route: &str) -> u8 {
	match route {
		REVIEW_ROUTE_CURRENT_BLOCKER => 0,
		REVIEW_ROUTE_LANDING_BLOCKER => 1,
		REVIEW_ROUTE_CONTRACT_OR_AUTHORITY_DECISION_REQUIRED => 2,
		REVIEW_ROUTE_NEEDS_EVIDENCE => 3,
		REVIEW_ROUTE_DETERMINISTIC_GATE_CANDIDATE => 4,
		REVIEW_ROUTE_ARCHITECTURE_SIGNAL => 5,
		REVIEW_ROUTE_ISSUE_CONTRACT_GAP => 6,
		REVIEW_ROUTE_FOLLOW_UP => 7,
		REVIEW_ROUTE_RISK_NOTE => 8,
		REVIEW_ROUTE_REVIEWER_RUBRIC_GAP => 9,
		REVIEW_ROUTE_INVALID_OR_UNSUBSTANTIATED => 10,
		_ => u8::MAX,
	}
}
