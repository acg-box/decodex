use std::path::Path;

use crate::orchestrator::{
	self, GhPullRequestReviewStateInspector, IssueDispatchMode, IssueTracker, PreferredRunIdentity,
	Result, RunSummary, TargetIssueRunContext, run_cycle::target_issue,
};

pub(crate) fn target_issue_has_status_visible_review_repair<T>(
	context: &TargetIssueRunContext<'_, T>,
) -> Result<bool>
where
	T: IssueTracker,
{
	let target_issue_id = target_issue::resolve_target_issue_id(context.tracker, context.issue_id)?;
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(context.project.github().token_env_var().to_owned()),
		github_command_path: context.project.github().command_path().map(Path::to_path_buf),
	};

	Ok(orchestrator::build_post_review_lane_statuses(
		context.tracker,
		context.project,
		context.workflow,
		context.state_store,
		&review_state_inspector,
	)?
	.into_iter()
	.any(|lane| {
		lane.issue_id == target_issue_id
			&& orchestrator::post_review_lane_is_repair_candidate(&lane)
	}))
}

pub(crate) fn run_target_status_visible_review_repair_once<T>(
	context: TargetIssueRunContext<'_, T>,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	let target_issue_id = target_issue::resolve_target_issue_id(context.tracker, context.issue_id)?;
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(context.project.github().token_env_var().to_owned()),
		github_command_path: context.project.github().command_path().map(Path::to_path_buf),
	};
	let Some(_issue) =
		orchestrator::select_target_post_review_repair_issue_candidate_with_inspector(
			context.tracker,
			context.project,
			context.workflow,
			context.state_store,
			&target_issue_id,
			context.issue_id,
			&review_state_inspector,
		)?
	else {
		return Ok(None);
	};

	target_issue::run_target_issue_once(target_issue::target_issue_run_context_with_dispatch_mode(
		&context,
		IssueDispatchMode::ReviewRepair,
	))
}

pub(crate) fn target_issue_has_status_visible_closeout<T>(
	context: &TargetIssueRunContext<'_, T>,
) -> Result<bool>
where
	T: IssueTracker,
{
	let target_issue_id = target_issue::resolve_target_issue_id(context.tracker, context.issue_id)?;
	let completed_state = context.workflow.frontmatter().tracker().resolved_completed_state();
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(context.project.github().token_env_var().to_owned()),
		github_command_path: context.project.github().command_path().map(Path::to_path_buf),
	};

	Ok(orchestrator::build_post_review_lane_statuses(
		context.tracker,
		context.project,
		context.workflow,
		context.state_store,
		&review_state_inspector,
	)?
	.into_iter()
	.any(|lane| {
		lane.issue_id == target_issue_id
			&& orchestrator::post_review_lane_is_closeout_candidate(&lane, completed_state)
	}))
}

pub(crate) fn run_target_status_visible_closeout_once<T>(
	context: TargetIssueRunContext<'_, T>,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	let target_issue_id = target_issue::resolve_target_issue_id(context.tracker, context.issue_id)?;
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(context.project.github().token_env_var().to_owned()),
		github_command_path: context.project.github().command_path().map(Path::to_path_buf),
	};
	let Some(candidate) =
		orchestrator::select_target_post_review_closeout_issue_candidate_with_inspector(
			context.tracker,
			context.project,
			context.workflow,
			context.state_store,
			&target_issue_id,
			context.issue_id,
			&review_state_inspector,
		)?
	else {
		return Ok(None);
	};
	let preferred_run_identity =
		candidate.preferred_run_identity.as_ref().map(|identity| PreferredRunIdentity {
			run_id: identity.run_id.as_str(),
			attempt_number: identity.attempt_number,
		});

	target_issue::run_target_issue_once(TargetIssueRunContext {
		tracker: context.tracker,
		project: context.project,
		workflow: context.workflow,
		state_store: context.state_store,
		issue_id: context.issue_id,
		preferred_issue_state: context.preferred_issue_state,
		preferred_initial_issue_state: context.preferred_initial_issue_state,
		dry_run: context.dry_run,
		lease_preacquired: context.lease_preacquired,
		preferred_issue_claim_fd: context.preferred_issue_claim_fd,
		preferred_dispatch_slot_fd: context.preferred_dispatch_slot_fd,
		preferred_dispatch_slot_index: context.preferred_dispatch_slot_index,
		dispatch_mode: IssueDispatchMode::Closeout,
		preferred_run_identity,
		preferred_retry_budget_base: context.preferred_retry_budget_base,
	})
}
