use crate::{
	manual::LandExecutionMode,
	prelude::{Result, eyre},
	pull_request::{self, LandingGateMode, PullRequestLandingGateView, PullRequestLandingState},
};

pub(in crate::manual) fn validate_landing_state(
	landing_state: &PullRequestLandingState,
	pr_url: &str,
	expected_base_branch: &str,
	current_branch: &str,
	current_head: &str,
) -> Result<LandExecutionMode> {
	let gate_view = landing_state.gate_view();

	if landing_state.base_ref_name != expected_base_branch {
		eyre::bail!(
			"Pull request `{pr_url}` targets base branch `{}`, but `decodex land` only lands into `{expected_base_branch}`.",
			landing_state.base_ref_name
		);
	}
	if landing_state.head_ref_name != current_branch {
		eyre::bail!(
			"Pull request `{pr_url}` points at branch `{}`, but the current branch is `{current_branch}`.",
			landing_state.head_ref_name
		);
	}
	if landing_state.head_ref_oid != current_head {
		eyre::bail!(
			"Pull request `{pr_url}` points at head `{}`, but the current branch head is `{current_head}`.",
			landing_state.head_ref_oid
		);
	}

	let decision = pull_request::classify_landing_gate(gate_view, LandingGateMode::ManualLand);

	match decision {
		pull_request::LandingGateDecision::Satisfied => {
			debug_assert!(pull_request::manual_landing_gates_satisfied(gate_view));

			Ok(LandExecutionMode::MergeAndCloseout)
		},
		pull_request::LandingGateDecision::CloseoutOnly => Ok(LandExecutionMode::CloseoutOnly),
		decision => manual_landing_gate_error(decision, gate_view, pr_url),
	}
}

fn manual_landing_gate_error(
	decision: pull_request::LandingGateDecision,
	gate_view: PullRequestLandingGateView<'_>,
	pr_url: &str,
) -> Result<LandExecutionMode> {
	match decision {
		pull_request::LandingGateDecision::Satisfied => Ok(LandExecutionMode::MergeAndCloseout),
		pull_request::LandingGateDecision::CloseoutOnly => Ok(LandExecutionMode::CloseoutOnly),
		pull_request::LandingGateDecision::Block("pull_request_not_open") => {
			eyre::bail!("Pull request `{pr_url}` is `{}` and cannot be landed.", gate_view.state)
		},
		pull_request::LandingGateDecision::Block("pull_request_is_draft") => {
			eyre::bail!("Pull request `{pr_url}` is still draft.")
		},
		pull_request::LandingGateDecision::Wait("pending_review_requests") => {
			eyre::bail!(
				"Pull request `{pr_url}` still has {} pending review request(s).",
				gate_view.pending_review_requests
			)
		},
		pull_request::LandingGateDecision::Repair("unresolved_review_threads") => {
			eyre::bail!(
				"Pull request `{pr_url}` still has {} unresolved review thread(s).",
				gate_view.unresolved_review_threads
			)
		},
		pull_request::LandingGateDecision::Repair("review_changes_requested") => {
			eyre::bail!("Pull request `{pr_url}` still has active change requests.")
		},
		pull_request::LandingGateDecision::Repair(reason)
			if matches!(
				reason,
				"pull_request_merge_conflict" | "pull_request_branch_behind_base"
			) =>
		{
			eyre::bail!("Pull request `{pr_url}` requires review repair: {reason}.")
		},
		pull_request::LandingGateDecision::Repair("required_checks_failed") => {
			eyre::bail!("Pull request `{pr_url}` has failed required checks that need repair.")
		},
		pull_request::LandingGateDecision::Wait("checks_waiting") => {
			let check_state = gate_view.status_check_rollup_state.unwrap_or("unknown");

			eyre::bail!(
				"Pull request `{pr_url}` is still waiting on checks: statusCheckRollup=`{check_state}`."
			)
		},
		pull_request::LandingGateDecision::Wait("mergeability_unknown") => {
			eyre::bail!(
				"Pull request `{pr_url}` mergeability is still unknown after retry; wait for GitHub to recompute mergeability and retry `decodex land`."
			)
		},
		pull_request::LandingGateDecision::Block("merge_state_not_ready") => {
			eyre::bail!(
				"Pull request `{pr_url}` is not ready to land: mergeStateStatus=`{}`.",
				gate_view.merge_state_status
			)
		},
		pull_request::LandingGateDecision::Block("not_mergeable") => {
			eyre::bail!(
				"Pull request `{pr_url}` is not mergeable: mergeable=`{}`.",
				gate_view.mergeable
			)
		},
		pull_request::LandingGateDecision::Wait("checks_non_green") => {
			let check_state = gate_view.status_check_rollup_state.unwrap_or("unknown");

			eyre::bail!(
				"Pull request `{pr_url}` still has non-green checks: statusCheckRollup=`{check_state}`."
			)
		},
		pull_request::LandingGateDecision::Wait(reason)
		| pull_request::LandingGateDecision::Repair(reason)
		| pull_request::LandingGateDecision::Block(reason) => {
			eyre::bail!("Pull request `{pr_url}` is not ready to land: {reason}.")
		},
	}
}
