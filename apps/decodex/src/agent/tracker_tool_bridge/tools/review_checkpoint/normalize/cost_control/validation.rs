use crate::agent::tracker_tool_bridge::{
	NormalizedReviewCheckpointContract, NormalizedReviewCheckpointFinding,
	NormalizedReviewCheckpointFindingRoute, NormalizedReviewCostControl, ReviewPolicyPhase,
	ReviewPolicyStatus,
	tools::review_checkpoint::{
		MAX_COMPACT_REVIEW_CHANGED_SURFACE_COUNT, REVIEW_CLASS_COMPACT_CURRENT_HEAD,
		REVIEW_CLASS_FULL_CURRENT_HEAD, REVIEW_ROUTE_CURRENT_BLOCKER,
		finding_policy::ReviewFindingPolicyUpdate, routes,
	},
};

pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint::normalize) fn validate_review_cost_control_for_checkpoint(
	cost_control: &NormalizedReviewCostControl,
	review_policy_phase: ReviewPolicyPhase,
	status: ReviewPolicyStatus,
	review_contract: &NormalizedReviewCheckpointContract,
	accepted_findings: &[NormalizedReviewCheckpointFinding],
	finding_routes: &[NormalizedReviewCheckpointFindingRoute],
) -> Result<(), String> {
	if cost_control.review_class == REVIEW_CLASS_FULL_CURRENT_HEAD {
		return Ok(());
	}

	let mut forced_full_reasons = compact_review_forced_full_reasons(
		cost_control,
		review_policy_phase,
		status,
		review_contract,
		accepted_findings,
		finding_routes,
	);

	if forced_full_reasons.is_empty() {
		return Ok(());
	}

	forced_full_reasons.sort();
	forced_full_reasons.dedup();

	let review_class = REVIEW_CLASS_COMPACT_CURRENT_HEAD;

	Err(format!(
		"`issue_review_checkpoint` cannot record `review_cost_control.review_class = {review_class}` because full review is required: {}.",
		forced_full_reasons.join(", ")
	))
}

pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint::normalize) fn validate_review_cost_control_policy_state(
	cost_control: &NormalizedReviewCostControl,
	policy_update: &ReviewFindingPolicyUpdate,
) -> Result<(), String> {
	if cost_control.review_class != REVIEW_CLASS_COMPACT_CURRENT_HEAD
		|| policy_update.previous_nonclean_rounds == 0
	{
		return Ok(());
	}

	let review_class = REVIEW_CLASS_COMPACT_CURRENT_HEAD;

	Err(format!(
		"`issue_review_checkpoint` cannot record `review_cost_control.review_class = {review_class}` because full review is required: prior_nonclean_review_rounds_present."
	))
}

fn compact_review_forced_full_reasons(
	cost_control: &NormalizedReviewCostControl,
	review_policy_phase: ReviewPolicyPhase,
	status: ReviewPolicyStatus,
	review_contract: &NormalizedReviewCheckpointContract,
	accepted_findings: &[NormalizedReviewCheckpointFinding],
	finding_routes: &[NormalizedReviewCheckpointFindingRoute],
) -> Vec<&'static str> {
	let mut reasons = Vec::new();

	if review_policy_phase != ReviewPolicyPhase::Handoff {
		reasons.push("repair_review_phase");
	}
	if status != ReviewPolicyStatus::Clean {
		reasons.push("nonclean_review_status");
	}
	if review_contract.risk_tier != "low" {
		reasons.push("review_contract_risk_tier_not_low");
	}
	if cost_control.risk_class != "low" {
		reasons.push("review_cost_risk_class_not_low");
	}
	if cost_control.changed_surface_count == 0 {
		reasons.push("missing_changed_surface_count");
	}
	if cost_control.changed_surface_count > MAX_COMPACT_REVIEW_CHANGED_SURFACE_COUNT {
		reasons.push("changed_surface_count_exceeds_compact_limit");
	}
	if !cost_control.high_risk_surfaces.is_empty() {
		reasons.push("high_risk_surfaces_present");
	}
	if !cost_control.current_head_evidence {
		reasons.push("missing_current_head_evidence");
	}
	if !cost_control.validation_backed {
		reasons.push("missing_validation_evidence");
	}
	if !cost_control.validation_current {
		reasons.push("stale_validation_evidence");
	}
	if !cost_control.evidence_sufficient {
		reasons.push("weak_evidence");
	}
	if !accepted_findings.is_empty() {
		reasons.push("accepted_findings_present");
	}
	if finding_routes.iter().any(|route| {
		route.route == REVIEW_ROUTE_CURRENT_BLOCKER || routes::review_route_blocks_landing(route)
	}) {
		reasons.push("blocking_finding_routes_present");
	}

	reasons
}
