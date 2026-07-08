use crate::orchestrator::{
	CONTINUATION_PENDING_RUN_STATUS, IssueDispatchMode, IssueTracker, PreferredRunIdentity, Result,
	RunSummary, TargetIssueRunContext, run_attempt_allows_continuation_reentry,
	run_cycle::target_issue::{self, post_review, program},
};

pub(crate) fn run_target_issue_once_with_inferred_dispatch<T>(
	context: TargetIssueRunContext<'_, T>,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	if post_review::target_issue_has_status_visible_review_repair(&context)? {
		return post_review::run_target_status_visible_review_repair_once(context);
	}
	if post_review::target_issue_has_status_visible_closeout(&context)? {
		return post_review::run_target_status_visible_closeout_once(context);
	}
	if let Some(summary) = run_target_status_visible_continuation_once(&context)? {
		return Ok(Some(summary));
	}

	if let Some(summary) = program::run_target_status_visible_program_once(
		target_issue::target_issue_run_context_with_dispatch_mode(
			&context,
			IssueDispatchMode::Program,
		),
	)? {
		return Ok(Some(summary));
	}
	if let Some(summary) = target_issue::run_target_issue_once(
		target_issue::target_issue_run_context_with_dispatch_mode(
			&context,
			IssueDispatchMode::Normal,
		),
	)? {
		return Ok(Some(summary));
	}
	if let Some(summary) = target_issue::run_target_issue_once(
		target_issue::target_issue_run_context_with_dispatch_mode(
			&context,
			IssueDispatchMode::Retry,
		),
	)? {
		return Ok(Some(summary));
	}
	if let Some(summary) = post_review::run_target_status_visible_review_repair_once(
		target_issue::target_issue_run_context_with_dispatch_mode(
			&context,
			IssueDispatchMode::ReviewRepair,
		),
	)? {
		return Ok(Some(summary));
	}

	post_review::run_target_status_visible_closeout_once(context)
}

fn run_target_status_visible_continuation_once<T>(
	context: &TargetIssueRunContext<'_, T>,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	let issue_id = target_issue::resolve_target_issue_id(context.tracker, context.issue_id)?;
	let Some(latest_attempt) = context.state_store.latest_run_attempt_for_issue(&issue_id)? else {
		return Ok(None);
	};

	if latest_attempt.status() != CONTINUATION_PENDING_RUN_STATUS
		&& !run_attempt_allows_continuation_reentry(
			context.project,
			context.state_store,
			&issue_id,
			&latest_attempt,
		)? {
		return Ok(None);
	}

	let in_progress = context.workflow.frontmatter().tracker().in_progress_state();
	let preferred_run_identity = PreferredRunIdentity {
		run_id: latest_attempt.run_id(),
		attempt_number: latest_attempt.attempt_number(),
	};

	target_issue::run_target_issue_once(TargetIssueRunContext {
		dispatch_mode: IssueDispatchMode::Retry,
		preferred_issue_state: Some(in_progress),
		preferred_initial_issue_state: None,
		preferred_run_identity: Some(preferred_run_identity),
		preferred_retry_budget_base: Some(
			context.state_store.retry_budget_attempt_count(&issue_id)?,
		),
		tracker: context.tracker,
		project: context.project,
		workflow: context.workflow,
		state_store: context.state_store,
		issue_id: context.issue_id,
		dry_run: context.dry_run,
		lease_preacquired: context.lease_preacquired,
		preferred_issue_claim_fd: context.preferred_issue_claim_fd,
		preferred_dispatch_slot_fd: context.preferred_dispatch_slot_fd,
		preferred_dispatch_slot_index: context.preferred_dispatch_slot_index,
	})
}
