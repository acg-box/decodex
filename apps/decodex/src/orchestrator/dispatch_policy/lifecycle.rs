use std::path::Path;

use crate::git_credentials::GitCredentialSource;
use crate::{
	default_branch_sync, github,
	orchestrator::{
		IssueDispatchMode, IssueRunPlan, IssueTracker, Result, ServiceConfig, StateStore,
		TrackerIssue, WorkflowDocument, delete_local_branch_if_present,
		detach_worktree_head_from_branch_if_checked_out, issue_passes_dispatch_policy,
		issue_passes_review_repair_dispatch_policy,
		ordinary_dispatch_blocked_by_retained_review_handoff, tracker,
	},
	prelude::eyre,
	state::{self, WorktreeMapping},
	worktree::WorktreeManager,
};

pub(in crate::orchestrator) fn clear_recovered_issue_lease(
	project_id: &str,
	issue_id: &str,
	expected_run_id: Option<&str>,
	state_store: &StateStore,
) -> Result<()> {
	let Some(lease) = state_store.lease_for_issue(issue_id)? else {
		return Ok(());
	};

	if lease.project_id() != project_id {
		return Ok(());
	}
	if expected_run_id.is_some_and(|run_id| lease.run_id() != run_id) {
		return Ok(());
	}

	state_store.clear_lease(issue_id)
}

pub(in crate::orchestrator) fn is_issue_eligible<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project_id: &str,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	let queue_label = tracker::automation_queue_label(project_id);

	if !issue_passes_dispatch_policy(tracker, issue, workflow, &queue_label, true)? {
		return Ok(false);
	}
	if ordinary_dispatch_blocked_by_retained_review_handoff(project_id, issue, state_store)? {
		return Ok(false);
	}

	Ok(state_store.lease_for_issue(&issue.id)?.is_none())
}

pub(in crate::orchestrator) fn todo_blocker_rule_passes(
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
) -> bool {
	if issue.state.name != "Todo" {
		return true;
	}

	issue.blockers.iter().all(|blocker| state_name_is_terminal(&blocker.state.name, workflow))
}

pub(in crate::orchestrator) fn refresh_issue<T>(
	tracker: &T,
	issue_id: &str,
) -> Result<Option<TrackerIssue>>
where
	T: IssueTracker,
{
	let issue_ids = [issue_id.to_owned()];
	let mut refreshed_issues = tracker.refresh_issues(&issue_ids)?;

	Ok(refreshed_issues.pop())
}

pub(in crate::orchestrator) fn is_terminal_issue(
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
) -> bool {
	state_name_is_terminal(&issue.state.name, workflow)
}

pub(in crate::orchestrator) fn state_name_is_terminal(
	state_name: &str,
	workflow: &WorkflowDocument,
) -> bool {
	workflow.frontmatter().tracker().terminal_states().iter().any(|state| state == state_name)
}

pub(in crate::orchestrator) fn is_issue_in_progress_for_run(
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
) -> bool {
	let tracker_policy = workflow.frontmatter().tracker();

	issue.state.name == tracker_policy.in_progress_state()
		&& !issue.has_label(tracker_policy.needs_attention_label())
}

pub(in crate::orchestrator) fn is_issue_not_dispatchable_for_run(
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
) -> bool {
	let tracker_policy = workflow.frontmatter().tracker();

	issue.has_label(tracker_policy.opt_out_label())
		|| issue.has_label(tracker_policy.needs_attention_label())
		|| (issue.state.name != tracker_policy.in_progress_state()
			&& !tracker_policy.startable_states().iter().any(|state| state == &issue.state.name))
}

pub(in crate::orchestrator) fn is_issue_not_dispatchable_for_current_dispatch<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	dispatch_mode: IssueDispatchMode,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	match dispatch_mode {
		IssueDispatchMode::ReviewRepair => {
			Ok(!issue_passes_review_repair_dispatch_policy(tracker, issue, project, workflow)?)
		},
		IssueDispatchMode::Normal
		| IssueDispatchMode::Program
		| IssueDispatchMode::Retry
		| IssueDispatchMode::Closeout => Ok(is_issue_not_dispatchable_for_run(issue, workflow)),
	}
}

pub(in crate::orchestrator) fn mark_run_attempt_if_active(
	state_store: &StateStore,
	run_id: &str,
	reconciled_status: &str,
) -> Result<()> {
	let Some(run_attempt) = state_store.run_attempt(run_id)? else {
		return Ok(());
	};

	if matches!(run_attempt.status(), "starting" | "running") {
		state_store.update_run_status(run_id, reconciled_status)?;
	}

	Ok(())
}

pub(in crate::orchestrator) fn cleanup_worktree_mapping(
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
	workflow: &WorkflowDocument,
	issue_identifier: &str,
	mapping: &WorktreeMapping,
) -> Result<()> {
	worktree_manager.remove_worktree_path_with_hooks(
		issue_identifier,
		mapping.branch_name(),
		mapping.worktree_path(),
		workflow.frontmatter().execution().workspace_hooks(),
	)?;
	state_store.clear_worktree(mapping.issue_id())?;

	Ok(())
}

pub(in crate::orchestrator) fn cleanup_terminal_worktree(
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
	workflow: &WorkflowDocument,
	issue_id: &str,
	issue_identifier: &str,
	branch_name: &str,
	worktree_path: &Path,
) -> Result<()> {
	worktree_manager.remove_worktree_path_with_hooks(
		issue_identifier,
		branch_name,
		worktree_path,
		workflow.frontmatter().execution().workspace_hooks(),
	)?;
	state_store.clear_worktree(issue_id)?;

	Ok(())
}

pub(in crate::orchestrator) fn clear_worktree_retry_schedule(
	state_store: &StateStore,
	issue_id: &str,
) -> Result<()> {
	let Some(worktree) = state_store.worktree_for_issue(issue_id)? else {
		return Ok(());
	};

	state::clear_run_retry_schedule(worktree.worktree_path())
}

pub(in crate::orchestrator) fn cleanup_completed_post_review_lane(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<()> {
	let worktree_manager =
		WorktreeManager::new(project.service_id(), project.repo_root(), project.worktree_root());
	let review_handoff = state_store
		.review_handoff_marker(
			project.service_id(),
			&issue_run.issue.id,
			&issue_run.worktree.branch_name,
		)?
		.ok_or_else(|| {
			eyre::eyre!(
				"Retained closeout cleanup for issue `{}` requires an existing runtime review handoff.",
				issue_run.issue.identifier
			)
		})?;
	let default_branch =
		review_handoff.target_base_ref_name().ok_or_else(|| {
			eyre::eyre!(
				"Retained closeout cleanup for issue `{}` requires the review handoff marker to record the PR target base branch.",
				issue_run.issue.identifier
			)
		})?;
	let github_token = project.github().resolve_token()?;
	let landing_state = github::inspect_pull_request_landing_state(
		&issue_run.worktree.path,
		review_handoff.pr_url(),
		&github_token,
		project.github().command_path(),
	)?;

	if landing_state.state != "MERGED" {
		eyre::bail!(
			"Retained closeout cleanup for issue `{}` requires PR `{}` to be merged, but GitHub reports `{}`.",
			issue_run.issue.identifier,
			review_handoff.pr_url(),
			landing_state.state
		);
	}
	if landing_state.base_ref_name != default_branch {
		eyre::bail!(
			"Retained closeout cleanup for issue `{}` expected PR `{}` target branch `{}`, but GitHub reports `{}`. Re-run review handoff/repair before cleanup.",
			issue_run.issue.identifier,
			review_handoff.pr_url(),
			default_branch,
			landing_state.base_ref_name
		);
	}

	let git_credentials = GitCredentialSource::new(project.github().token_env_var(), &github_token);

	default_branch_sync::sync_repo_root_default_branch(
		project.repo_root(),
		default_branch,
		Some(git_credentials),
	)?;
	github::delete_pull_request_head_branch_if_present(
		project.repo_root(),
		review_handoff.pr_url(),
		&issue_run.worktree.branch_name,
		&github_token,
		project.github().command_path(),
	)?;

	detach_worktree_head_from_branch_if_checked_out(
		&issue_run.worktree.path,
		&issue_run.worktree.branch_name,
	)?;
	delete_local_branch_if_present(project.repo_root(), &issue_run.worktree.branch_name)?;

	worktree_manager.remove_worktree_path_with_hooks(
		&issue_run.issue.identifier,
		&issue_run.worktree.branch_name,
		&issue_run.worktree.path,
		workflow.frontmatter().execution().workspace_hooks(),
	)?;
	state_store.clear_worktree(&issue_run.issue.id)?;

	Ok(())
}
