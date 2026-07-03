use std::{path::Path, thread};

use crate::{
	github,
	manual::{
		self, LandExecutionMode, MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT,
		MANUAL_LAND_MERGEABILITY_RETRY_ATTEMPTS, MANUAL_LAND_MERGEABILITY_RETRY_DELAY,
		ManualLandContext,
	},
	prelude::{Result, eyre},
	pull_request::{self, LandingGateMode, PullRequestLandingGateView, PullRequestLandingState},
};

pub(super) fn inspect_pull_request_landing_state_for_manual_land(
	cwd: &Path,
	pr_url: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<PullRequestLandingState> {
	let mut last_landing_state = None;

	for attempt in 1..=MANUAL_LAND_MERGEABILITY_RETRY_ATTEMPTS {
		let landing_state =
			github::inspect_pull_request_landing_state(cwd, pr_url, github_token, gh_command_path)?;

		if landing_state.state == "MERGED"
			|| !pull_request::mergeability_unknown(landing_state.gate_view())
		{
			return Ok(landing_state);
		}

		last_landing_state = Some(landing_state);

		if attempt < MANUAL_LAND_MERGEABILITY_RETRY_ATTEMPTS {
			tracing::info!(
				pr_url = %pr_url,
				attempt,
				mergeable = "UNKNOWN",
				merge_state_status = "UNKNOWN",
				"Pull request mergeability is unresolved; waiting for GitHub to recompute before validating manual land gates."
			);

			thread::sleep(MANUAL_LAND_MERGEABILITY_RETRY_DELAY);
		}
	}

	last_landing_state
		.ok_or_else(|| eyre::eyre!("Pull request `{pr_url}` landing state was unavailable."))
}

pub(super) fn execute_land_merge(
	context: &ManualLandContext,
	current_head: &str,
	landed_change_record: &str,
	execution_mode: LandExecutionMode,
) -> Result<String> {
	match execution_mode {
		LandExecutionMode::MergeAndCloseout => {
			manual::ensure_clean_worktree(&context.cwd)?;

			if !context.repository.merge_commit_allowed {
				eyre::bail!(
					"GitHub repository `{}/{}` does not allow merge commits, but `decodex land` requires an admin merge commit.",
					context.repository.owner,
					context.repository.name
				);
			}

			if let Err(error) = github::admin_merge_pull_request(
				&context.canonical_repo_root,
				&context.pr_url,
				current_head,
				Some(landed_change_record),
				&context.github_token,
				context.github_command_path.as_deref(),
			) {
				if matches!(
					github::pull_request_is_merged_at_head(
						&context.canonical_repo_root,
						&context.pr_url,
						current_head,
						&context.github_token,
						context.github_command_path.as_deref(),
					),
					Ok(true)
				) {
					return github::wait_for_pull_request_merge_commit(
						&context.canonical_repo_root,
						&context.pr_url,
						&context.github_token,
						MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT,
						context.github_command_path.as_deref(),
					);
				}

				return Err(error);
			}
		},
		LandExecutionMode::CloseoutOnly => {},
	}

	github::wait_for_pull_request_merge_commit(
		&context.canonical_repo_root,
		&context.pr_url,
		&context.github_token,
		MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT,
		context.github_command_path.as_deref(),
	)
}

pub(super) fn load_authoritative_landed_change_record(
	context: &ManualLandContext,
	merge_commit: &str,
) -> Result<String> {
	github::wait_for_commit_subject(
		&context.canonical_repo_root,
		&context.pr_url,
		merge_commit,
		&context.github_token,
		MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT,
		context.github_command_path.as_deref(),
	)
}

pub(super) fn validate_landing_state(
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

pub(super) fn manual_landing_gate_error(
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
