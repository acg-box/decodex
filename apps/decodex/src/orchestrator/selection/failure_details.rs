use color_eyre::Report;

use crate::orchestrator::{
	self, AgentGitCredentialsUnavailable, AppServerCapabilityPreflightFailure,
	AppServerDynamicToolFailure, AppServerHomePreflightFailure, AppServerPhaseGoalFailure,
	AppServerTransportFailure, AppServerTurnFailure, AppServerZeroEvidenceStartFailure,
	CodexAccountAuthFailure, LoopGuardrailReason, LoopGuardrailStopRequested,
	ManualAttentionRequested, RepoGateFailure, RepoGateFailureDisposition, RetainedPartialProgress,
	RetainedReviewNeedsAttention, RetainedReviewRepairPushFailed, ReviewHandoffNeedsAttention,
	ReviewPolicyStopReason, ReviewPolicyStopRequested, StalledRunNeedsAttention,
};

pub(crate) fn retry_comment_details(error: &Report) -> (&'static str, String) {
	debug_assert!(
		!orchestrator::run_failure_writeback_disposition(error).requires_terminal_attention(),
		"terminal-attention failures must not be formatted as retry comments"
	);

	if let Some(repo_gate_failure) = error.downcast_ref::<RepoGateFailure>() {
		match repo_gate_failure.disposition() {
			RepoGateFailureDisposition::ContinueRepair
			| RepoGateFailureDisposition::RetryAfterBackoff => {
				return (
					repo_gate_failure.error_class(),
					repo_gate_failure.retry_next_action().to_owned(),
				);
			},
			RepoGateFailureDisposition::NeedsHumanAttention => {},
		}
	}
	if let Some(app_server_failure) = error.downcast_ref::<AppServerZeroEvidenceStartFailure>() {
		return (app_server_failure.error_class(), app_server_failure.retry_next_action());
	}
	if let Some(app_server_failure) = error.downcast_ref::<AppServerCapabilityPreflightFailure>()
		&& app_server_failure.is_retryable_timeout()
	{
		return (app_server_failure.error_class(), app_server_failure.retry_next_action());
	}
	if let Some(app_server_failure) = error.downcast_ref::<AppServerTransportFailure>()
		&& app_server_failure.is_retryable_startup()
	{
		return (app_server_failure.error_class(), app_server_failure.retry_next_action());
	}
	if let Some(app_server_failure) = error.downcast_ref::<AppServerPhaseGoalFailure>()
		&& app_server_failure.is_terminal_path_missing()
	{
		return (app_server_failure.error_class(), app_server_failure.retry_next_action());
	}
	if let Some(app_server_failure) = error.downcast_ref::<AppServerDynamicToolFailure>() {
		return (app_server_failure.error_class(), app_server_failure.retry_next_action());
	}

	if error.downcast_ref::<StalledRunNeedsAttention>().is_some() {
		return (
			"stalled_run_detected",
			String::from(
				"decodex will retry the stalled lane automatically; inspect the worktree and app-server activity if the retry budget exhausts",
			),
		);
	}

	if let Some(app_server_failure) = error.downcast_ref::<AppServerTurnFailure>()
		&& app_server_failure.is_retryable_capacity_failure()
	{
		return (
			app_server_failure.error_class(),
			app_server_failure.retry_next_action().to_owned(),
		);
	}

	("retryable_execution_failure", String::from("decodex will retry automatically"))
}

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
		let error_class =
			retained_review_needs_attention_error_class(&retained_review_needs_attention.reason);

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
			review_policy_stop_terminal_next_action(review_policy_stop.reason, recovery_gate),
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

pub(crate) fn review_policy_stop_terminal_next_action(
	reason: ReviewPolicyStopReason,
	recovery_gate: &str,
) -> String {
	match reason {
		ReviewPolicyStopReason::Exhausted => format!(
			"inspect the repeated review findings and current worktree, decide the next repair or redesign manually, prepare a bounded convergence research follow-up only after the current head, review phase, non-clean round count, and validated findings are structured and machine-checkable, {recovery_gate}"
		),
		ReviewPolicyStopReason::ArchitectureReviewRequired => format!(
			"inspect the current findings and worktree, perform the required architecture review manually, prepare a bounded architecture research follow-up only after the current head, review phase, stop class, and architecture concern are structured and machine-checkable, {recovery_gate}"
		),
		ReviewPolicyStopReason::Blocked => format!(
			"inspect the blocking condition and worktree, resolve the blocker manually, do not dispatch research unless the blocker is reclassified as a structured architecture or convergence stop, {recovery_gate}"
		),
	}
}

pub(crate) fn retained_review_needs_attention_error_class(reason: &str) -> &'static str {
	match reason {
		"external_review_admin_merge_failed" => "external_review_admin_merge_failed",
		"external_review_admin_merge_unavailable" => "external_review_admin_merge_unavailable",
		"external_review_merge_visibility_timeout" => "external_review_merge_visibility_timeout",
		"external_review_pass_signal_missing" => "external_review_pass_signal_missing",
		"external_review_request_ci_red_manual_attention" =>
			"external_review_request_ci_red_manual_attention",
		"non_github_review_admin_merge_failed" => "non_github_review_admin_merge_failed",
		"non_github_review_admin_merge_unavailable" => "non_github_review_admin_merge_unavailable",
		"non_github_review_merge_visibility_timeout" =>
			"non_github_review_merge_visibility_timeout",
		"pull_request_is_draft" => "pull_request_is_draft",
		"pull_request_merge_commit_lineage_check_failed" =>
			"pull_request_merge_commit_lineage_check_failed",
		"pull_request_not_open" => "pull_request_not_open",
		"retained_admin_merge_subject_unavailable" => "retained_admin_merge_subject_unavailable",
		"review_orchestration_branch_mismatch" => "review_orchestration_branch_mismatch",
		"review_orchestration_head_mismatch" => "review_orchestration_head_mismatch",
		"review_orchestration_pr_mismatch" => "review_orchestration_pr_mismatch",
		"worktree_head_missing" => "worktree_head_missing",
		_ => "retained_review_needs_attention",
	}
}

pub(crate) fn terminal_failure_recovery_gate(
	needs_attention_label: &str,
	needs_attention_label_available: bool,
	guarded_by_nonstartable_state: bool,
	nonstartable_guard_state: &str,
) -> String {
	if needs_attention_label_available {
		return format!(
			"clear label `{needs_attention_label}`, then move the issue back to a startable state if another automated run is desired"
		);
	}
	if guarded_by_nonstartable_state {
		return format!(
			"`{needs_attention_label}` could not be applied because it does not exist on the team; the issue remains in `{nonstartable_guard_state}` to block automatic retries, so move it back to a startable state manually if another automated run is desired"
		);
	}

	format!(
		"`{needs_attention_label}` could not be applied because it does not exist on the team; move the issue back to a startable state manually if another automated run is desired"
	)
}
