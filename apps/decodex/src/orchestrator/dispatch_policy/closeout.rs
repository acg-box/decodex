use std::path::Path;

use crate::{
	orchestrator::{
		CloseoutDispatchEligibility, GhPullRequestReviewStateInspector, IssueTracker,
		PullRequestReviewStateInspector, Result, RetainedCloseoutPrMergeGate, ServiceConfig,
		StateStore, TrackerIssue, WorkflowDocument, issue_has_service_ownership,
		retained_closeout_pr_merge_gate_with_inspector,
	},
	worktree::{WorktreeManager, WorktreeSpec},
};

pub(in crate::orchestrator) fn issue_passes_review_repair_dispatch_policy<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	let tracker_policy = workflow.frontmatter().tracker();

	Ok(issue_has_service_ownership(tracker, issue, project.service_id())?
		&& issue.state.name == tracker_policy.success_state()
		&& !issue.has_label(tracker_policy.opt_out_label())
		&& !issue.has_label(tracker_policy.needs_attention_label()))
}

pub(in crate::orchestrator) fn issue_passes_closeout_dispatch_policy<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
		github_command_path: project.github().command_path().map(Path::to_path_buf),
	};

	issue_passes_closeout_dispatch_policy_with_inspector(
		tracker,
		issue,
		project,
		workflow,
		state_store,
		&review_state_inspector,
	)
}

pub(crate) fn issue_passes_closeout_dispatch_policy_with_inspector<T, I>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
	I: PullRequestReviewStateInspector + ?Sized,
{
	Ok(matches!(
		evaluate_closeout_dispatch_policy_with_inspector(
			tracker,
			issue,
			project,
			workflow,
			state_store,
			review_state_inspector,
		)?,
		CloseoutDispatchEligibility::Eligible
	))
}

pub(in crate::orchestrator) fn closeout_dispatch_block_reason<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<Option<&'static str>>
where
	T: IssueTracker + ?Sized,
{
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
		github_command_path: project.github().command_path().map(Path::to_path_buf),
	};

	closeout_dispatch_block_reason_with_inspector(
		tracker,
		issue,
		project,
		workflow,
		state_store,
		&review_state_inspector,
	)
}

pub(crate) fn closeout_dispatch_block_reason_with_inspector<T, I>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
) -> Result<Option<&'static str>>
where
	T: IssueTracker + ?Sized,
	I: PullRequestReviewStateInspector + ?Sized,
{
	Ok(
		match evaluate_closeout_dispatch_policy_with_inspector(
			tracker,
			issue,
			project,
			workflow,
			state_store,
			review_state_inspector,
		)? {
			CloseoutDispatchEligibility::Blocked(reason) => Some(reason),
			CloseoutDispatchEligibility::Eligible | CloseoutDispatchEligibility::Ineligible => None,
		},
	)
}

pub(in crate::orchestrator) fn evaluate_closeout_dispatch_policy_with_inspector<T, I>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
) -> Result<CloseoutDispatchEligibility>
where
	T: IssueTracker + ?Sized,
	I: PullRequestReviewStateInspector + ?Sized,
{
	let tracker_policy = workflow.frontmatter().tracker();
	let completed_state = tracker_policy.resolved_completed_state();
	let issue_state = issue.state.name.as_str();

	if issue.has_label(tracker_policy.opt_out_label())
		|| issue.has_label(tracker_policy.needs_attention_label())
	{
		return Ok(CloseoutDispatchEligibility::Ineligible);
	}
	if !issue_has_service_ownership(tracker, issue, project.service_id())? {
		return Ok(CloseoutDispatchEligibility::Ineligible);
	}
	if issue_state != tracker_policy.success_state() && issue_state != completed_state {
		return Ok(CloseoutDispatchEligibility::Ineligible);
	}

	let worktree = match state_store.worktree_for_issue(&issue.id)? {
		Some(mapping) => {
			if mapping.project_id() != project.service_id()
				|| !mapping.worktree_path().try_exists()?
			{
				return Ok(CloseoutDispatchEligibility::Ineligible);
			}

			WorktreeSpec {
				branch_name: mapping.branch_name().to_owned(),
				issue_identifier: issue.identifier.clone(),
				path: mapping.worktree_path().to_path_buf(),
				reused_existing: true,
			}
		},
		None => {
			let worktree_manager = WorktreeManager::new(
				project.service_id(),
				project.repo_root(),
				project.worktree_root(),
			);
			let planned_worktree = worktree_manager.plan_for_issue(&issue.identifier);
			if !planned_worktree.path.try_exists()? {
				return Ok(CloseoutDispatchEligibility::Ineligible);
			}

			planned_worktree
		},
	};

	let Some(review_handoff) = state_store.review_handoff_marker(
		project.service_id(),
		&issue.id,
		&worktree.branch_name,
	)?
	else {
		return Ok(CloseoutDispatchEligibility::Blocked("missing_review_handoff_record"));
	};

	if review_handoff.branch_name() != worktree.branch_name {
		return Ok(CloseoutDispatchEligibility::Ineligible);
	}

	Ok(
		match retained_closeout_pr_merge_gate_with_inspector(
			&worktree.path,
			&worktree.branch_name,
			review_handoff.pr_url(),
			review_state_inspector,
		)? {
			RetainedCloseoutPrMergeGate::Merged => CloseoutDispatchEligibility::Eligible,
			RetainedCloseoutPrMergeGate::NotMerged => {
				CloseoutDispatchEligibility::Blocked("pull_request_not_merged")
			},
			RetainedCloseoutPrMergeGate::PullRequestStateReadFailed => {
				CloseoutDispatchEligibility::Blocked("pull_request_state_read_failed")
			},
		},
	)
}
