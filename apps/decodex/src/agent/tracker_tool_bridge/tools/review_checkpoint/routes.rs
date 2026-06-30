use std::collections::{BTreeMap, BTreeSet};

use crate::agent::tracker_tool_bridge::{
	self, NormalizedRejectedReviewCheckpointFinding, NormalizedReviewCheckpointFinding,
	NormalizedReviewCheckpointFindingRoute, NormalizedReviewCheckpointPayload,
	ReviewCheckpointFindingRouteArgs, ReviewCheckpointFindingRouteCount,
	ReviewCheckpointFindingRouteSummary,
};

use super::{
	REVIEW_ROUTE_ARCHITECTURE_SIGNAL, REVIEW_ROUTE_CONTRACT_OR_AUTHORITY_DECISION_REQUIRED,
	REVIEW_ROUTE_CURRENT_BLOCKER, REVIEW_ROUTE_DETERMINISTIC_GATE_CANDIDATE,
	REVIEW_ROUTE_FOLLOW_UP, REVIEW_ROUTE_INVALID_OR_UNSUBSTANTIATED,
	REVIEW_ROUTE_ISSUE_CONTRACT_GAP, REVIEW_ROUTE_LANDING_BLOCKER, REVIEW_ROUTE_NEEDS_EVIDENCE,
	REVIEW_ROUTE_REVIEWER_RUBRIC_GAP, REVIEW_ROUTE_RISK_HIGH, REVIEW_ROUTE_RISK_NOTE,
	REVIEW_ROUTE_SOURCE_ACCEPTED, REVIEW_ROUTE_SOURCE_REJECTED, REVIEW_ROUTE_SOURCE_ROUTE_ONLY,
	normalize::{
		normalize_required_review_evidence_list, normalize_required_review_text,
		normalize_review_severity,
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
		let route =
			normalize_review_checkpoint_finding_route(route, accepted_findings, rejected_findings)?;

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
			routes.push(default_current_blocker_route(index, finding));
		}
	}
	for (index, finding) in rejected_findings.iter().enumerate() {
		let index = u64::try_from(index).map_err(|error| {
			format!("Failed to normalize rejected finding route index: {error}")
		})?;

		if !explicitly_routed_rejected.contains(&index) {
			routes.push(default_reviewer_rubric_gap_route(index, finding));
		}
	}

	Ok(routes)
}

fn normalize_review_checkpoint_finding_route(
	route: ReviewCheckpointFindingRouteArgs,
	accepted_findings: &[NormalizedReviewCheckpointFinding],
	rejected_findings: &[NormalizedRejectedReviewCheckpointFinding],
) -> Result<NormalizedReviewCheckpointFindingRoute, String> {
	let route_name = normalize_review_finding_route_name(route.route)?;
	let severity = normalize_review_severity(route.severity, "finding_routes.severity")?;
	let risk_tier = normalize_review_route_risk_tier(route.risk_tier)?;
	let summary = normalize_required_review_text(route.summary, "finding_routes.summary")?;
	let evidence =
		normalize_required_review_evidence_list(route.evidence, "finding_routes.evidence")?;
	let resolver = normalize_required_review_text(route.resolver, "finding_routes.resolver")?;
	let next_action =
		normalize_required_review_text(route.next_action, "finding_routes.next_action")?;
	let finding_source = normalize_review_route_source(route.finding_source)?;
	let (finding_index, finding_fingerprint) = normalize_review_route_binding(
		&finding_source,
		route.finding_index,
		accepted_findings,
		rejected_findings,
	)?;
	let bound_finding_high_severity = review_route_bound_finding_severity(
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

fn default_current_blocker_route(
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

fn default_reviewer_rubric_gap_route(
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

fn normalize_review_route_binding(
	source: &str,
	index: Option<u64>,
	accepted_findings: &[NormalizedReviewCheckpointFinding],
	rejected_findings: &[NormalizedRejectedReviewCheckpointFinding],
) -> Result<(Option<u64>, Option<String>), String> {
	match source {
		REVIEW_ROUTE_SOURCE_ACCEPTED => {
			let index = index.ok_or_else(|| {
				String::from(
					"`finding_routes.finding_index` is required when `finding_source` is `accepted_findings`.",
				)
			})?;
			let finding = accepted_findings
				.get(usize::try_from(index).map_err(|error| {
					format!("Failed to normalize accepted finding route index: {error}")
				})?)
				.ok_or_else(|| {
					format!(
						"`finding_routes.finding_index` `{index}` does not match any accepted finding."
					)
				})?;

			Ok((Some(index), Some(finding.fingerprint.clone())))
		},
		REVIEW_ROUTE_SOURCE_REJECTED => {
			let index = index.ok_or_else(|| {
				String::from(
					"`finding_routes.finding_index` is required when `finding_source` is `rejected_findings`.",
				)
			})?;

			rejected_findings
				.get(usize::try_from(index).map_err(|error| {
					format!("Failed to normalize rejected finding route index: {error}")
				})?)
				.ok_or_else(|| {
					format!(
						"`finding_routes.finding_index` `{index}` does not match any rejected finding."
					)
				})?;

			Ok((Some(index), None))
		},
		REVIEW_ROUTE_SOURCE_ROUTE_ONLY => {
			if index.is_some() {
				return Err(String::from(
					"`finding_routes.finding_index` is only valid with `accepted_findings` or `rejected_findings` sources.",
				));
			}

			Ok((None, None))
		},
		_ => Err(String::from(
			"`finding_routes.finding_source` did not normalize to a supported source.",
		)),
	}
}

fn review_route_bound_finding_severity<'a>(
	source: &str,
	index: Option<u64>,
	accepted_findings: &'a [NormalizedReviewCheckpointFinding],
	rejected_findings: &'a [NormalizedRejectedReviewCheckpointFinding],
) -> Option<&'a str> {
	let index = usize::try_from(index?).ok()?;

	match source {
		REVIEW_ROUTE_SOURCE_ACCEPTED =>
			accepted_findings.get(index).map(|finding| finding.severity.as_str()),
		REVIEW_ROUTE_SOURCE_REJECTED =>
			rejected_findings.get(index).map(|finding| finding.severity.as_str()),
		_ => None,
	}
}

fn review_severity_blocks_invalid_route(severity: &str) -> bool {
	matches!(severity, "critical" | "high")
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
		next_action: review_route_next_action(routes),
	}
}

fn review_route_next_action(routes: &[NormalizedReviewCheckpointFindingRoute]) -> Option<String> {
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

pub(super) fn current_review_blocker_routes(
	routes: &[NormalizedReviewCheckpointFindingRoute],
) -> impl Iterator<Item = &NormalizedReviewCheckpointFindingRoute> {
	routes.iter().filter(|route| route.route == REVIEW_ROUTE_CURRENT_BLOCKER)
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
