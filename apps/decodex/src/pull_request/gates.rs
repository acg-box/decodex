use crate::pull_request::{LandingGateDecision, LandingGateMode, PullRequestLandingGateView};

pub(crate) fn classify_landing_gate(
	view: PullRequestLandingGateView<'_>,
	mode: LandingGateMode,
) -> LandingGateDecision {
	if view.state == "MERGED" {
		return if mode.allows_closeout_only() {
			LandingGateDecision::CloseoutOnly
		} else {
			LandingGateDecision::Block("pull_request_not_open")
		};
	}
	if view.state != "OPEN" {
		return LandingGateDecision::Block("pull_request_not_open");
	}
	if view.is_draft {
		return LandingGateDecision::Block("pull_request_is_draft");
	}
	if mode.requires_review_requests_clear() && view.pending_review_requests > 0 {
		return LandingGateDecision::Wait("pending_review_requests");
	}
	if mode.requires_review_threads_clear() && view.unresolved_review_threads > 0 {
		return LandingGateDecision::Repair("unresolved_review_threads");
	}
	if view.review_decision == Some("CHANGES_REQUESTED") {
		return LandingGateDecision::Repair("review_changes_requested");
	}

	if let Some(reason) =
		merge_state_requires_review_repair(view.mergeable, view.merge_state_status)
	{
		return LandingGateDecision::Repair(reason);
	}

	if let Some(decision) = required_status_context_gate(view) {
		return decision;
	}

	if !has_required_status_contexts(view)
		&& failed_checks_require_repair(view.status_check_rollup_state, view.merge_state_status)
	{
		return LandingGateDecision::Repair("required_checks_failed");
	}

	if !has_required_status_contexts(view)
		&& let Some(check_state) = view.status_check_rollup_state
		&& checks_require_wait(Some(check_state))
	{
		return LandingGateDecision::Wait("checks_waiting");
	}

	if mergeability_unknown(view) {
		return LandingGateDecision::Wait("mergeability_unknown");
	}
	if !merge_state_allows_ready_to_land_for_view(view) {
		return LandingGateDecision::Block("merge_state_not_ready");
	}
	if view.mergeable != "MERGEABLE" {
		return LandingGateDecision::Block("not_mergeable");
	}
	if mode.requires_green_status_rollup()
		&& !has_required_status_contexts(view)
		&& !matches!(view.status_check_rollup_state, None | Some("SUCCESS"))
	{
		return LandingGateDecision::Wait("checks_non_green");
	}

	LandingGateDecision::Satisfied
}

pub(crate) fn manual_landing_gates_satisfied(view: PullRequestLandingGateView<'_>) -> bool {
	classify_landing_gate(view, LandingGateMode::ManualLand) == LandingGateDecision::Satisfied
}

pub(crate) fn retained_landing_gates_satisfied(view: PullRequestLandingGateView<'_>) -> bool {
	classify_landing_gate(view, LandingGateMode::Retained) == LandingGateDecision::Satisfied
}

pub(crate) fn retained_clean_path_landing_gates_satisfied(
	view: PullRequestLandingGateView<'_>,
) -> bool {
	retained_landing_gates_satisfied(view)
		&& merge_state_allows_clean_path_landing(view)
		&& (has_required_status_contexts(view)
			|| matches!(view.status_check_rollup_state, None | Some("SUCCESS")))
}

pub(crate) fn retained_landing_requires_agent_fallback(
	view: PullRequestLandingGateView<'_>,
) -> bool {
	let configured_status_contexts_ready =
		has_required_status_contexts(view) && required_status_context_gate(view).is_none();
	let legacy_rollup_ready = !has_required_status_contexts(view)
		&& !checks_require_wait(view.status_check_rollup_state)
		&& !failed_checks_require_repair(view.status_check_rollup_state, view.merge_state_status);
	let review_and_check_gates_ready = view.state == "OPEN"
		&& !view.is_draft
		&& view.review_decision != Some("CHANGES_REQUESTED")
		&& view.unresolved_review_threads == 0
		&& (configured_status_contexts_ready || legacy_rollup_ready);

	review_and_check_gates_ready
		&& ((retained_landing_gates_satisfied(view)
			&& !retained_clean_path_landing_gates_satisfied(view))
			|| mergeability_unknown(view))
}

pub(crate) fn mergeability_unknown(view: PullRequestLandingGateView<'_>) -> bool {
	view.mergeable == "UNKNOWN" || view.merge_state_status == "UNKNOWN"
}

pub(crate) fn merge_state_allows_ready_to_land(merge_state_status: &str) -> bool {
	matches!(merge_state_status, "CLEAN" | "HAS_HOOKS" | "UNSTABLE")
}

fn merge_state_allows_ready_to_land_for_view(view: PullRequestLandingGateView<'_>) -> bool {
	merge_state_allows_ready_to_land(view.merge_state_status)
		|| (view.fast_landing && view.merge_state_status == "BLOCKED")
}

fn merge_state_allows_clean_path_landing(view: PullRequestLandingGateView<'_>) -> bool {
	view.merge_state_status == "CLEAN"
		|| (view.fast_landing
			&& matches!(view.merge_state_status, "BLOCKED" | "HAS_HOOKS" | "UNSTABLE"))
}

pub(crate) fn checks_require_wait(check_state: Option<&str>) -> bool {
	matches!(check_state, Some("EXPECTED" | "PENDING"))
}

pub(crate) fn failed_checks_require_repair(
	check_state: Option<&str>,
	merge_state_status: &str,
) -> bool {
	matches!(check_state, Some("ERROR" | "FAILURE")) && merge_state_status == "BLOCKED"
}

pub(crate) fn merge_state_requires_review_repair(
	mergeable: &str,
	merge_state_status: &str,
) -> Option<&'static str> {
	if mergeable == "CONFLICTING" {
		return Some("pull_request_merge_conflict");
	}
	if merge_state_status == "BEHIND" {
		return Some("pull_request_branch_behind_base");
	}

	None
}

fn has_required_status_contexts(view: PullRequestLandingGateView<'_>) -> bool {
	!view.required_status_contexts.is_empty()
}

fn required_status_context_gate(
	view: PullRequestLandingGateView<'_>,
) -> Option<LandingGateDecision> {
	if !has_required_status_contexts(view) {
		return None;
	}
	if view.required_status_contexts.iter().any(|context| context.state.is_none()) {
		return Some(LandingGateDecision::Wait("required_status_context_missing"));
	}
	if view.required_status_contexts.iter().any(|context| !context.allowed_creator) {
		return Some(LandingGateDecision::Block("required_status_context_creator_mismatch"));
	}
	if view.required_status_contexts.iter().any(|context| {
		matches!(context.state.as_deref(), Some("failure" | "FAILURE" | "error" | "ERROR"))
	}) {
		return Some(LandingGateDecision::Repair("required_status_context_failed"));
	}
	if view
		.required_status_contexts
		.iter()
		.any(|context| !matches!(context.state.as_deref(), Some("success" | "SUCCESS")))
	{
		return Some(LandingGateDecision::Wait("required_status_context_waiting"));
	}
	if view.required_status_contexts.iter().any(|context| !context.base_ref_matches) {
		return Some(LandingGateDecision::Wait("required_status_context_base_stale"));
	}

	None
}
