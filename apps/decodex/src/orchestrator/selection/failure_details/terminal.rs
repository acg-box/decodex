use color_eyre::Report;

use crate::orchestrator::{
	AgentGitCredentialsUnavailable, AppServerCapabilityPreflightFailure,
	AppServerDynamicToolFailure, AppServerHomePreflightFailure, AppServerPhaseGoalFailure,
	AppServerTransportFailure, AppServerTurnFailure, AppServerZeroEvidenceStartFailure,
	CodexAccountAuthFailure, LoopGuardrailReason, LoopGuardrailStopRequested,
	ManualAttentionRequested, RepoGateFailure, RetainedPartialProgress,
	RetainedReviewNeedsAttention, RetainedReviewRepairPushFailed, ReviewHandoffNeedsAttention,
	ReviewPolicyStopRequested, StalledRunNeedsAttention, selection::failure_details::review,
};

pub(crate) fn terminal_failure_pr_url(error: &Report) -> Option<&str> {
	error.downcast_ref::<ReviewHandoffNeedsAttention>().map(|error| error.pr_url.as_str()).or_else(
		|| {
			error
				.downcast_ref::<RetainedReviewRepairPushFailed>()
				.and_then(|error| error.pr_url.as_deref())
		},
	)
}

pub(crate) fn terminal_failure_comment_details(
	manual_attention_requested: bool,
	error: &Report,
	recovery_gate: &str,
) -> (&'static str, String) {
	if let Some(retained_review_needs_attention) =
		error.downcast_ref::<RetainedReviewNeedsAttention>()
	{
		let error_class = review::retained_review_needs_attention_error_class(
			&retained_review_needs_attention.reason,
		);

		(
			error_class,
			format!(
				"inspect retained review orchestration reason `{}`, resolve the blocker manually, {recovery_gate}",
				retained_review_needs_attention.reason
			),
		)
	} else if let Some(loop_guardrail_stop) = error.downcast_ref::<LoopGuardrailStopRequested>() {
		(
			loop_guardrail_stop.terminal_error_class(),
			loop_guardrail_stop.terminal_next_action(recovery_gate),
		)
	} else if manual_attention_requested {
		if let Some(manual_attention) = error.downcast_ref::<ManualAttentionRequested>()
			&& let Some(error_class) = manual_attention.error_class.as_deref()
			&& let Some(reason) = LoopGuardrailReason::from_error_class(error_class)
		{
			return (reason.error_class(), reason.terminal_next_action(recovery_gate));
		}

		(
			"human_attention_required",
			format!(
				"inspect the issue comment and worktree, resolve the blocker manually, {recovery_gate}"
			),
		)
	} else if error.downcast_ref::<ReviewHandoffNeedsAttention>().is_some() {
		(
			"review_handoff_writeback_failed",
			format!(
				"inspect the tracker state, PR, and worktree, repair the incomplete review handoff manually, {recovery_gate}"
			),
		)
	} else if let Some(push_failure) = error.downcast_ref::<RetainedReviewRepairPushFailed>() {
		(push_failure.error_class(), push_failure.terminal_next_action(recovery_gate))
	} else if let Some(partial_progress) = error.downcast_ref::<RetainedPartialProgress>() {
		(
			"partial_progress_retained",
			format!(
				"inspect retained worktree `{}`, finish validation and PR handoff or reset the patch manually, {recovery_gate}",
				partial_progress.worktree_path
			),
		)
	} else if let Some(app_server_failure) =
		error.downcast_ref::<AppServerZeroEvidenceStartFailure>()
	{
		(app_server_failure.error_class(), app_server_failure.terminal_next_action(recovery_gate))
	} else if let Some(account_failure) = error.downcast_ref::<CodexAccountAuthFailure>() {
		(account_failure.error_class(), account_failure.terminal_next_action(recovery_gate))
	} else if error.downcast_ref::<StalledRunNeedsAttention>().is_some() {
		(
			"stalled_run_detected",
			format!(
				"inspect the worktree and app-server activity for the stalled lane, resolve the blocker manually, {recovery_gate}"
			),
		)
	} else if error.downcast_ref::<AgentGitCredentialsUnavailable>().is_some() {
		(
			"github_credentials_unavailable",
			format!(
				"repair GitHub authentication for this lane, verify noninteractive Git access, {recovery_gate}"
			),
		)
	} else if let Some(app_server_failure) =
		error.downcast_ref::<AppServerCapabilityPreflightFailure>()
	{
		(app_server_failure.error_class(), app_server_failure.terminal_next_action(recovery_gate))
	} else if let Some(app_server_failure) = error.downcast_ref::<AppServerHomePreflightFailure>() {
		(app_server_failure.error_class(), app_server_failure.terminal_next_action(recovery_gate))
	} else if let Some(app_server_failure) = error.downcast_ref::<AppServerTransportFailure>() {
		(app_server_failure.error_class(), app_server_failure.terminal_next_action(recovery_gate))
	} else if let Some(app_server_failure) = error.downcast_ref::<AppServerPhaseGoalFailure>() {
		(app_server_failure.error_class(), app_server_failure.terminal_next_action(recovery_gate))
	} else if let Some(app_server_failure) = error.downcast_ref::<AppServerDynamicToolFailure>() {
		(app_server_failure.error_class(), app_server_failure.terminal_next_action(recovery_gate))
	} else if let Some(app_server_failure) = error.downcast_ref::<AppServerTurnFailure>() {
		(app_server_failure.error_class(), app_server_failure.terminal_next_action(recovery_gate))
	} else if let Some(review_policy_stop) = error.downcast_ref::<ReviewPolicyStopRequested>() {
		(
			review_policy_stop.reason.error_class(),
			review::review_policy_stop_terminal_next_action(
				review_policy_stop.reason,
				recovery_gate,
			),
		)
	} else if let Some(repo_gate_failure) = error.downcast_ref::<RepoGateFailure>() {
		(repo_gate_failure.error_class(), repo_gate_failure.terminal_next_action(recovery_gate))
	} else {
		(
			"retry_budget_exhausted",
			format!("inspect the worktree, resolve the issue manually, {recovery_gate}"),
		)
	}
}
