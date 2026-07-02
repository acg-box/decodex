use crate::orchestrator::{
	IssueDispatchMode, IssueTracker, Result, RunSummary, TargetIssueRunContext,
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
