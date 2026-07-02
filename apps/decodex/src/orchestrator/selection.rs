use crate::orchestrator::*;
use crate::tracker;

pub(in crate::orchestrator) struct RetryComment<'a> {
	pub(in crate::orchestrator) run_id: &'a str,
	pub(in crate::orchestrator) attempt_number: i64,
	pub(in crate::orchestrator) retry_budget_attempt_number: i64,
	pub(in crate::orchestrator) max_attempts: i64,
	pub(in crate::orchestrator) worktree_path: String,
	pub(in crate::orchestrator) branch_name: &'a str,
	pub(in crate::orchestrator) error_class: &'a str,
	pub(in crate::orchestrator) next_action: &'a str,
}

#[cfg_attr(not(test), allow(dead_code))]
pub(in crate::orchestrator) fn select_issue_candidate(
	tracker: &dyn IssueTracker,
	issues: Vec<TrackerIssue>,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	project_id: &str,
) -> Result<Option<TrackerIssue>> {
	select_issue_candidate_with_exclusions(tracker, issues, workflow, state_store, project_id, &[])
}

pub(in crate::orchestrator) fn select_issue_candidate_with_exclusions(
	tracker: &dyn IssueTracker,
	issues: Vec<TrackerIssue>,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	project_id: &str,
	excluded_issue_ids: &[&str],
) -> Result<Option<TrackerIssue>> {
	let mut eligible_issues = Vec::new();

	for issue in issues {
		if excluded_issue_ids.contains(&issue.id.as_str()) {
			continue;
		}
		if state_store.issue_has_active_shared_claim(project_id, &issue.id)? {
			continue;
		}
		if is_issue_eligible(tracker, &issue, project_id, workflow, state_store)? {
			eligible_issues.push(issue);
		}
	}

	eligible_issues.sort_by(compare_issue_candidates);

	Ok(eligible_issues.into_iter().next())
}

pub(in crate::orchestrator) fn compare_issue_candidates(
	left: &TrackerIssue,
	right: &TrackerIssue,
) -> Ordering {
	let left_priority = (left.priority.is_none(), left.priority.unwrap_or(i64::MAX));
	let right_priority = (right.priority.is_none(), right.priority.unwrap_or(i64::MAX));

	left_priority
		.cmp(&right_priority)
		.then_with(|| left.created_at.cmp(&right.created_at))
		.then_with(|| left.identifier.cmp(&right.identifier))
}

pub(in crate::orchestrator) fn format_no_eligible_issue_message(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
) -> String {
	let tracker_policy = workflow.frontmatter().tracker();

	format!(
		"No eligible issue found for the configured project.\n{}",
		format_no_eligible_issue_hint(
			project.service_id(),
			tracker_policy.opt_out_label(),
			tracker_policy.needs_attention_label(),
		)
	)
}

pub(in crate::orchestrator) fn format_status_no_eligible_issue_hint(service_id: &str) -> String {
	format!(
		"Hint: check `Todo`, label {}, no opt-out/manual-only or needs-attention labels, non-terminal state, no open dependency blockers, and no active issue claim.",
		format_no_eligible_queue_label_hint(service_id),
	)
}

pub(in crate::orchestrator) fn format_no_eligible_issue_hint(
	service_id: &str,
	opt_out_label: &str,
	needs_attention_label: &str,
) -> String {
	format!(
		"Hint: check `Todo`, label {}, no `{opt_out_label}`/`{needs_attention_label}`, non-terminal state, no open dependency blockers, and no active issue claim.",
		format_no_eligible_queue_label_hint(service_id),
	)
}

pub(in crate::orchestrator) fn format_no_eligible_queue_label_hint(service_id: &str) -> String {
	let queue_label = tracker::automation_queue_label(service_id);

	if service_id == "all" {
		String::from("`decodex:queued:<service-id>`")
	} else {
		format!("`decodex:queued:<service-id>` (this project: `{queue_label}`)")
	}
}

pub(in crate::orchestrator) fn format_retry_comment(comment: RetryComment<'_>) -> String {
	let RetryComment {
		run_id,
		attempt_number,
		retry_budget_attempt_number,
		max_attempts,
		worktree_path,
		branch_name,
		error_class,
		next_action,
	} = comment;

	format!(
		"decodex run failed and will retry\n\n- run_id: `{run_id}`\n- run_sequence_attempt: `{attempt_number}` (not retry-budget count)\n- retry_budget_attempt: `{retry_budget_attempt_number}` / `{max_attempts}`\n- failed_at: `{failed_at}`\n- branch: `{branch}`\n- worktree_path: `{worktree}`\n- error_class: `{error_class}`\n- next_action: `{next_action}`\n- error_summary: `Sensitive runtime details were withheld from the tracker comment; inspect the local lane for the full failure context.`",
		failed_at = current_timestamp(),
		branch = branch_name,
		worktree = worktree_path,
	)
}

pub(in crate::orchestrator) fn retry_comment_details(error: &Report) -> (&'static str, String) {
	debug_assert!(
		!run_failure_writeback_disposition(error).requires_terminal_attention(),
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

pub(in crate::orchestrator) fn format_terminal_failure_comment(
	run_id: &str,
	attempt_number: i64,
	worktree_path: String,
	branch_name: &str,
	pr_url: Option<&str>,
	error_class: &str,
	next_action: &str,
) -> String {
	let pr_url_line = pr_url.map_or_else(String::new, |pr_url| format!("\n- pr_url: `{pr_url}`"));
	let retained_partial_progress = error_class == "partial_progress_retained";
	let heading = if retained_partial_progress {
		"decodex retained partial progress and needs attention"
	} else {
		"decodex run failed and needs attention"
	};
	let timestamp_label = if retained_partial_progress { "recorded_at" } else { "failed_at" };
	let error_summary = if retained_partial_progress {
		"Sensitive runtime details were withheld from the tracker comment; inspect the retained lane for the full recovery context."
	} else {
		"Sensitive runtime details were withheld from the tracker comment; inspect the local lane for the full failure context."
	};

	format!(
		"{heading}\n\n- run_id: `{run_id}`\n- run_sequence_attempt: `{attempt_number}` (not retry-budget count)\n- {timestamp_label}: `{timestamp}`\n- branch: `{branch}`{pr_url_line}\n- worktree_path: `{worktree}`\n- error_class: `{error_class}`\n- next_action: `{next_action}`\n- error_summary: `{error_summary}`",
		timestamp = current_timestamp(),
		branch = branch_name,
		worktree = worktree_path
	)
}

pub(in crate::orchestrator) fn terminal_failure_pr_url(error: &Report) -> Option<&str> {
	error.downcast_ref::<ReviewHandoffNeedsAttention>().map(|error| error.pr_url.as_str()).or_else(
		|| {
			error
				.downcast_ref::<RetainedReviewRepairPushFailed>()
				.and_then(|error| error.pr_url.as_deref())
		},
	)
}

pub(in crate::orchestrator) fn terminal_failure_comment_details(
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

pub(in crate::orchestrator) fn review_policy_stop_terminal_next_action(
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

pub(in crate::orchestrator) fn retained_review_needs_attention_error_class(
	reason: &str,
) -> &'static str {
	match reason {
		"external_review_admin_merge_failed" => "external_review_admin_merge_failed",
		"external_review_admin_merge_unavailable" => "external_review_admin_merge_unavailable",
		"external_review_merge_visibility_timeout" => "external_review_merge_visibility_timeout",
		"external_review_pass_signal_missing" => "external_review_pass_signal_missing",
		"external_review_request_ci_red_manual_attention" => {
			"external_review_request_ci_red_manual_attention"
		},
		"non_github_review_admin_merge_failed" => "non_github_review_admin_merge_failed",
		"non_github_review_admin_merge_unavailable" => "non_github_review_admin_merge_unavailable",
		"non_github_review_merge_visibility_timeout" => {
			"non_github_review_merge_visibility_timeout"
		},
		"pull_request_is_draft" => "pull_request_is_draft",
		"pull_request_merge_commit_lineage_check_failed" => {
			"pull_request_merge_commit_lineage_check_failed"
		},
		"pull_request_not_open" => "pull_request_not_open",
		"retained_admin_merge_subject_unavailable" => "retained_admin_merge_subject_unavailable",
		"review_orchestration_branch_mismatch" => "review_orchestration_branch_mismatch",
		"review_orchestration_head_mismatch" => "review_orchestration_head_mismatch",
		"review_orchestration_pr_mismatch" => "review_orchestration_pr_mismatch",
		"worktree_head_missing" => "worktree_head_missing",
		_ => "retained_review_needs_attention",
	}
}

pub(in crate::orchestrator) fn terminal_failure_recovery_gate(
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

pub(in crate::orchestrator) fn current_timestamp() -> String {
	OffsetDateTime::now_utc().format(&Rfc3339).expect("timestamp formatting should succeed")
}

pub(in crate::orchestrator) fn build_run_id(
	issue_identifier: &str,
	attempt_number: i64,
) -> Result<String> {
	let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

	Ok(format!("{}-attempt-{attempt_number}-{timestamp}", issue_identifier.to_lowercase()))
}

pub(in crate::orchestrator) fn resolve_config_path(
	explicit_path: Option<&Path>,
	state_store: &StateStore,
) -> Result<Option<PathBuf>> {
	if let Some(path) = explicit_path {
		return Ok(Some(path.to_path_buf()));
	}

	runtime::registered_config_path_for_cwd(state_store, &env::current_dir()?)
}

pub(in crate::orchestrator) fn sleep_until_next_tick(
	poll_interval: Duration,
	tick_started_at: Instant,
) {
	let elapsed = tick_started_at.elapsed();

	if elapsed < poll_interval {
		thread::sleep(poll_interval - elapsed);
	}
}
