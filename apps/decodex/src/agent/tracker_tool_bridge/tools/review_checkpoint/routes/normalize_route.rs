use crate::agent::tracker_tool_bridge::{
	self, NormalizedRejectedReviewCheckpointFinding, NormalizedReviewCheckpointFinding,
	NormalizedReviewCheckpointFindingRoute, ReviewCheckpointFindingRouteArgs,
	tools::review_checkpoint::{
		REVIEW_ROUTE_ARCHITECTURE_SIGNAL, REVIEW_ROUTE_CONTRACT_OR_AUTHORITY_DECISION_REQUIRED,
		REVIEW_ROUTE_CURRENT_BLOCKER, REVIEW_ROUTE_DETERMINISTIC_GATE_CANDIDATE,
		REVIEW_ROUTE_FOLLOW_UP, REVIEW_ROUTE_INVALID_OR_UNSUBSTANTIATED,
		REVIEW_ROUTE_ISSUE_CONTRACT_GAP, REVIEW_ROUTE_LANDING_BLOCKER, REVIEW_ROUTE_NEEDS_EVIDENCE,
		REVIEW_ROUTE_REVIEWER_RUBRIC_GAP, REVIEW_ROUTE_RISK_HIGH, REVIEW_ROUTE_RISK_NOTE,
		REVIEW_ROUTE_SOURCE_ACCEPTED, REVIEW_ROUTE_SOURCE_REJECTED, REVIEW_ROUTE_SOURCE_ROUTE_ONLY,
		normalize, routes::binding,
	},
};

pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint::routes) fn normalize_review_checkpoint_finding_route(
	route: ReviewCheckpointFindingRouteArgs,
	accepted_findings: &[NormalizedReviewCheckpointFinding],
	rejected_findings: &[NormalizedRejectedReviewCheckpointFinding],
) -> Result<NormalizedReviewCheckpointFindingRoute, String> {
	let route_name = normalize_review_finding_route_name(route.route)?;
	let severity = normalize::normalize_review_severity(route.severity, "finding_routes.severity")?;
	let risk_tier = normalize_review_route_risk_tier(route.risk_tier)?;
	let summary =
		normalize::normalize_required_review_text(route.summary, "finding_routes.summary")?;
	let evidence = normalize::normalize_required_review_evidence_list(
		route.evidence,
		"finding_routes.evidence",
	)?;
	let resolver =
		normalize::normalize_required_review_text(route.resolver, "finding_routes.resolver")?;
	let next_action =
		normalize::normalize_required_review_text(route.next_action, "finding_routes.next_action")?;
	let finding_source = normalize_review_route_source(route.finding_source)?;
	let (finding_index, finding_fingerprint) = binding::normalize_review_route_binding(
		&finding_source,
		route.finding_index,
		accepted_findings,
		rejected_findings,
	)?;
	let bound_finding_high_severity = binding::review_route_bound_finding_severity(
		&finding_source,
		finding_index,
		accepted_findings,
		rejected_findings,
	)
	.is_some_and(review_severity_blocks_invalid_route);

	if route_name == REVIEW_ROUTE_CURRENT_BLOCKER
		&& (finding_source != REVIEW_ROUTE_SOURCE_ACCEPTED || finding_fingerprint.is_none())
	{
		return Err(String::from(
			"`finding_routes.route` `current_blocker` must bind to an `accepted_findings` item with `finding_index`.",
		));
	}
	if route_name == REVIEW_ROUTE_INVALID_OR_UNSUBSTANTIATED
		&& (review_severity_blocks_invalid_route(severity.as_str())
			|| bound_finding_high_severity
			|| risk_tier == REVIEW_ROUTE_RISK_HIGH)
	{
		return Err(String::from(
			"`issue_review_checkpoint` cannot route high-severity or high-risk `finding_routes` items to `invalid_or_unsubstantiated`; use `needs_evidence` or a landing-blocking route.",
		));
	}

	Ok(NormalizedReviewCheckpointFindingRoute {
		route: route_name,
		severity,
		risk_tier,
		summary,
		evidence,
		resolver,
		next_action,
		finding_source,
		finding_index,
		finding_fingerprint,
	})
}

fn normalize_review_finding_route_name(route: String) -> Result<String, String> {
	let route = route.trim().to_ascii_lowercase().replace([' ', '-'], "_");

	match route.as_str() {
		REVIEW_ROUTE_CURRENT_BLOCKER
		| REVIEW_ROUTE_LANDING_BLOCKER
		| REVIEW_ROUTE_CONTRACT_OR_AUTHORITY_DECISION_REQUIRED
		| REVIEW_ROUTE_NEEDS_EVIDENCE
		| REVIEW_ROUTE_FOLLOW_UP
		| REVIEW_ROUTE_DETERMINISTIC_GATE_CANDIDATE
		| REVIEW_ROUTE_ARCHITECTURE_SIGNAL
		| REVIEW_ROUTE_ISSUE_CONTRACT_GAP
		| REVIEW_ROUTE_REVIEWER_RUBRIC_GAP
		| REVIEW_ROUTE_RISK_NOTE
		| REVIEW_ROUTE_INVALID_OR_UNSUBSTANTIATED => Ok(route),
		other => Err(format!(
			"`finding_routes.route` must be one of the supported Decodex Review route taxonomy values, not `{other}`."
		)),
	}
}

fn normalize_review_route_risk_tier(risk_tier: Option<String>) -> Result<String, String> {
	let Some(risk_tier) = tracker_tool_bridge::normalize_optional_progress_field(risk_tier) else {
		return Ok(String::from("low"));
	};
	let risk_tier = risk_tier.to_ascii_lowercase().replace([' ', '-'], "_");

	match risk_tier.as_str() {
		"low" | "medium" | REVIEW_ROUTE_RISK_HIGH => Ok(risk_tier),
		other => Err(format!(
			"`finding_routes.risk_tier` must be `low`, `medium`, or `high`, not `{other}`."
		)),
	}
}

fn normalize_review_route_source(source: Option<String>) -> Result<String, String> {
	let Some(source) = tracker_tool_bridge::normalize_optional_progress_field(source) else {
		return Ok(String::from(REVIEW_ROUTE_SOURCE_ROUTE_ONLY));
	};
	let source = source.to_ascii_lowercase().replace([' ', '-'], "_");

	match source.as_str() {
		REVIEW_ROUTE_SOURCE_ACCEPTED
		| REVIEW_ROUTE_SOURCE_REJECTED
		| REVIEW_ROUTE_SOURCE_ROUTE_ONLY => Ok(source),
		other => Err(format!(
			"`finding_routes.finding_source` must be `accepted_findings`, `rejected_findings`, or `route_only`, not `{other}`."
		)),
	}
}

fn review_severity_blocks_invalid_route(severity: &str) -> bool {
	matches!(severity, "critical" | "high")
}
