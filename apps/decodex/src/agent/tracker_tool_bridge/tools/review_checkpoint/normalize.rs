pub(super) mod shared;

mod checks;
mod contract;
mod cost_control;
mod findings;

pub(super) use self::{
	findings::normalize_review_checkpoint_finding,
	shared::{
		normalize_required_review_evidence_list, normalize_required_review_text,
		normalize_review_severity,
	},
};

use crate::agent::tracker_tool_bridge::{
	ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, LocalRepoDetails, NormalizedReviewCheckpointPayload,
	NormalizedReviewCostControl, ReviewCheckpointArgs, ReviewCheckpointHeadBinding,
	ReviewPolicyPhase, ReviewPolicyStatus,
	tools::review_checkpoint::{
		INDEPENDENT_FRESH_CONTEXT_REVIEWER, REVIEW_ROUTE_CURRENT_BLOCKER,
		REVIEW_ROUTE_SOURCE_ACCEPTED, finding_policy, finding_policy::ReviewFindingPolicyUpdate,
		routes,
	},
};

pub(in crate::agent::tracker_tool_bridge::tools) fn normalize_review_checkpoint_payload(
	parsed: ReviewCheckpointArgs,
	review_policy_phase: ReviewPolicyPhase,
	status: ReviewPolicyStatus,
	head_sha: &str,
	local_repo: &LocalRepoDetails,
) -> Result<NormalizedReviewCheckpointPayload, String> {
	let reviewer = parsed
		.reviewer
		.map(|reviewer| reviewer.trim().to_owned())
		.filter(|reviewer| !reviewer.is_empty())
		.ok_or_else(|| {
			format!(
				"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `reviewer` set to `{}`.",
				INDEPENDENT_FRESH_CONTEXT_REVIEWER
			)
		})?;

	if reviewer != INDEPENDENT_FRESH_CONTEXT_REVIEWER {
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` reviewer must be `{}`, not `{reviewer}`.",
			INDEPENDENT_FRESH_CONTEXT_REVIEWER
		));
	}

	let review_contract = contract::normalize_review_checkpoint_contract(
		parsed.review_contract.ok_or_else(|| {
			format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `review_contract`.")
		})?,
		review_policy_phase,
	)?;
	let review_contract_hash = contract::review_checkpoint_contract_hash(&review_contract)?;
	let review_cost_control =
		cost_control::normalize_review_cost_control(parsed.review_cost_control, &review_contract)?;
	let checks = checks::normalize_review_checkpoint_checks(
		parsed
			.checks
			.ok_or_else(|| format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `checks`."))?,
	)?;
	let evidence = normalize_required_review_evidence_list(parsed.evidence, "evidence")?;
	let accepted_findings = parsed
		.accepted_findings
		.into_iter()
		.map(|finding| normalize_review_checkpoint_finding(finding, review_policy_phase))
		.collect::<Result<Vec<_>, _>>()?;
	let rejected_findings = parsed
		.rejected_findings
		.into_iter()
		.map(findings::normalize_rejected_review_checkpoint_finding)
		.collect::<Result<Vec<_>, _>>()?;
	let finding_routes = routes::normalize_review_checkpoint_finding_routes(
		parsed.finding_routes,
		&accepted_findings,
		&rejected_findings,
	)?;
	let finding_route_summary = routes::summarize_review_checkpoint_finding_routes(&finding_routes);

	cost_control::validate_review_cost_control_for_checkpoint(
		&review_cost_control,
		review_policy_phase,
		status,
		&review_contract,
		&accepted_findings,
		&finding_routes,
	)?;

	if status == ReviewPolicyStatus::Findings
		&& !routes::current_review_blocker_routes(&finding_routes).any(|route| {
			route.finding_source == REVIEW_ROUTE_SOURCE_ACCEPTED
				&& route.finding_fingerprint.is_some()
		}) {
		return Err(String::from(
			"`issue_review_checkpoint` status `findings` requires at least one accepted finding routed as `current_blocker`. Route non-current comments through `finding_routes` and use `clean` when no current repair remains.",
		));
	}
	if status == ReviewPolicyStatus::Clean && !accepted_findings.is_empty() {
		return Err(String::from(
			"`issue_review_checkpoint` status `clean` cannot include accepted findings. Reject non-actionable comments explicitly or use status `findings` for accepted repair work.",
		));
	}
	if status == ReviewPolicyStatus::Clean
		&& finding_routes.iter().any(|route| {
			route.route == REVIEW_ROUTE_CURRENT_BLOCKER
				|| routes::review_route_blocks_landing(route)
		}) {
		return Err(String::from(
			"`issue_review_checkpoint` status `clean` can record only non-blocking `finding_routes` such as `follow_up`, `risk_note`, `reviewer_rubric_gap`, or `invalid_or_unsubstantiated`.",
		));
	}
	if matches!(status, ReviewPolicyStatus::Blocked | ReviewPolicyStatus::NeedsArchitectureReview)
		&& !finding_routes.iter().any(routes::review_route_blocks_landing)
	{
		return Err(String::from(
			"`issue_review_checkpoint` status `blocked` or `needs_architecture_review` requires at least one landing-blocking `finding_routes` item with evidence, resolver, and machine-actionable next_action.",
		));
	}

	Ok(NormalizedReviewCheckpointPayload {
		reviewer,
		review_contract,
		review_contract_hash,
		review_cost_control,
		reviewed_head: ReviewCheckpointHeadBinding {
			head_sha: head_sha.to_owned(),
			head_tree_oid: local_repo.head_tree_oid.clone(),
			review_worktree_clean: local_repo.review_worktree_clean(),
		},
		checks,
		evidence,
		accepted_findings,
		rejected_findings,
		finding_routes,
		finding_route_summary,
		finding_policy: finding_policy::empty_review_finding_policy(
			review_policy_phase,
			status,
			head_sha,
		),
	})
}

pub(in crate::agent::tracker_tool_bridge::tools) fn validate_review_cost_control_policy_state(
	cost_control: &NormalizedReviewCostControl,
	policy_update: &ReviewFindingPolicyUpdate,
) -> Result<(), String> {
	cost_control::validate_review_cost_control_policy_state(cost_control, policy_update)
}
