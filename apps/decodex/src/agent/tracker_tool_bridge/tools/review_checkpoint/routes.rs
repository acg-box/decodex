mod binding;
mod defaults;
mod normalize_route;
mod summary;

use std::collections::{BTreeMap, BTreeSet};

use crate::agent::tracker_tool_bridge::{
	NormalizedRejectedReviewCheckpointFinding, NormalizedReviewCheckpointFinding,
	NormalizedReviewCheckpointFindingRoute, NormalizedReviewCheckpointPayload,
	ReviewCheckpointFindingRouteArgs, ReviewCheckpointFindingRouteCount,
	ReviewCheckpointFindingRouteSummary,
	tools::review_checkpoint::{
		REVIEW_ROUTE_ARCHITECTURE_SIGNAL, REVIEW_ROUTE_CONTRACT_OR_AUTHORITY_DECISION_REQUIRED,
		REVIEW_ROUTE_CURRENT_BLOCKER, REVIEW_ROUTE_DETERMINISTIC_GATE_CANDIDATE,
		REVIEW_ROUTE_ISSUE_CONTRACT_GAP, REVIEW_ROUTE_LANDING_BLOCKER, REVIEW_ROUTE_NEEDS_EVIDENCE,
		REVIEW_ROUTE_SOURCE_ACCEPTED, REVIEW_ROUTE_SOURCE_REJECTED,
	},
};

pub(super) fn normalize_review_checkpoint_finding_routes(
	explicit_routes: Vec<ReviewCheckpointFindingRouteArgs>,
	accepted_findings: &[NormalizedReviewCheckpointFinding],
	rejected_findings: &[NormalizedRejectedReviewCheckpointFinding],
) -> Result<Vec<NormalizedReviewCheckpointFindingRoute>, String> {
	let mut routes = Vec::new();
	let mut explicitly_routed_accepted = BTreeSet::new();
	let mut explicitly_routed_rejected = BTreeSet::new();

	for route in explicit_routes {
		let route = normalize_route::normalize_review_checkpoint_finding_route(
			route,
			accepted_findings,
			rejected_findings,
		)?;

		if route.finding_source == REVIEW_ROUTE_SOURCE_ACCEPTED
			&& let Some(index) = route.finding_index
		{
			explicitly_routed_accepted.insert(index);
		} else if route.finding_source == REVIEW_ROUTE_SOURCE_REJECTED
			&& let Some(index) = route.finding_index
		{
			explicitly_routed_rejected.insert(index);
		}

		routes.push(route);
	}
	for (index, finding) in accepted_findings.iter().enumerate() {
		let index = u64::try_from(index).map_err(|error| {
			format!("Failed to normalize accepted finding route index: {error}")
		})?;

		if !explicitly_routed_accepted.contains(&index) {
			routes.push(defaults::default_current_blocker_route(index, finding));
		}
	}
	for (index, finding) in rejected_findings.iter().enumerate() {
		let index = u64::try_from(index).map_err(|error| {
			format!("Failed to normalize rejected finding route index: {error}")
		})?;

		if !explicitly_routed_rejected.contains(&index) {
			routes.push(defaults::default_reviewer_rubric_gap_route(index, finding));
		}
	}

	Ok(routes)
}

pub(super) fn summarize_review_checkpoint_finding_routes(
	routes: &[NormalizedReviewCheckpointFindingRoute],
) -> ReviewCheckpointFindingRouteSummary {
	let mut counts = BTreeMap::<String, usize>::new();

	for route in routes {
		*counts.entry(route.route.clone()).or_default() += 1;
	}

	ReviewCheckpointFindingRouteSummary {
		route_counts: counts
			.into_iter()
			.map(|(route, count)| ReviewCheckpointFindingRouteCount { route, count })
			.collect(),
		next_action: summary::review_route_next_action(routes),
	}
}

pub(super) fn current_review_blocker_routes(
	routes: &[NormalizedReviewCheckpointFindingRoute],
) -> impl Iterator<Item = &NormalizedReviewCheckpointFindingRoute> {
	routes.iter().filter(|route| route.route == REVIEW_ROUTE_CURRENT_BLOCKER)
}

pub(super) fn review_route_blocks_landing(route: &NormalizedReviewCheckpointFindingRoute) -> bool {
	matches!(
		route.route.as_str(),
		REVIEW_ROUTE_LANDING_BLOCKER
			| REVIEW_ROUTE_CONTRACT_OR_AUTHORITY_DECISION_REQUIRED
			| REVIEW_ROUTE_NEEDS_EVIDENCE
			| REVIEW_ROUTE_DETERMINISTIC_GATE_CANDIDATE
			| REVIEW_ROUTE_ARCHITECTURE_SIGNAL
			| REVIEW_ROUTE_ISSUE_CONTRACT_GAP
	)
}

pub(in crate::agent::tracker_tool_bridge::tools) fn current_review_blocker_findings(
	payload: &NormalizedReviewCheckpointPayload,
) -> impl Iterator<Item = &NormalizedReviewCheckpointFinding> {
	let fingerprints = current_review_blocker_routes(&payload.finding_routes)
		.filter_map(|route| route.finding_fingerprint.clone())
		.collect::<BTreeSet<_>>();

	payload
		.accepted_findings
		.iter()
		.filter(move |finding| fingerprints.contains(&finding.fingerprint))
}
