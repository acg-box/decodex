use crate::{
	orchestrator::{
		dispatch_policy,
		dispatch_policy::{
			GitCredentialSource, IssueRunPlan, Path, Result, ServiceConfig, StateStore,
			WorkflowDocument, WorktreeManager, WorktreeMapping, default_branch_sync, eyre, github,
		},
	},
	state,
};

pub(crate) fn cleanup_worktree_mapping(
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

pub(crate) fn cleanup_terminal_worktree(
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

pub(crate) fn clear_worktree_retry_schedule(
	state_store: &StateStore,
	issue_id: &str,
) -> Result<()> {
	let Some(worktree) = state_store.worktree_for_issue(issue_id)? else {
		return Ok(());
	};

	state::clear_run_retry_schedule(worktree.worktree_path())
}

pub(crate) fn cleanup_completed_post_review_lane(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<()> {
	let worktree_manager =
		WorktreeManager::new(project.service_id(), project.repo_root(), project.worktree_root());
	let lifecycle_record = state_store
		.review_lifecycle_record(
			project.service_id(),
			&issue_run.issue.id,
			&issue_run.worktree.branch_name,
		)?
		.ok_or_else(|| {
			eyre::eyre!(
				"Retained closeout cleanup for issue `{}` requires an existing runtime review lifecycle authority.",
				issue_run.issue.identifier
			)
		})?;
	let default_branch =
		lifecycle_record.target_base_ref_name().ok_or_else(|| {
			eyre::eyre!(
				"Retained closeout cleanup for issue `{}` requires the review lifecycle authority to record the PR target base branch.",
				issue_run.issue.identifier
			)
		})?;
	let github_token = project.github().resolve_token()?;
	let landing_state = github::inspect_pull_request_landing_state(
		&issue_run.worktree.path,
		lifecycle_record.pr_url(),
		&github_token,
		project.github().command_path(),
		project.github().landing_required_status_contexts(),
		project.github().landing_required_status_creators(),
	)?;

	if landing_state.state != "MERGED" {
		eyre::bail!(
			"Retained closeout cleanup for issue `{}` requires PR `{}` to be merged, but GitHub reports `{}`.",
			issue_run.issue.identifier,
			lifecycle_record.pr_url(),
			landing_state.state
		);
	}
	if landing_state.base_ref_name != default_branch {
		eyre::bail!(
			"Retained closeout cleanup for issue `{}` expected PR `{}` target branch `{}`, but GitHub reports `{}`. Re-run review handoff/repair before cleanup.",
			issue_run.issue.identifier,
			lifecycle_record.pr_url(),
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
		lifecycle_record.pr_url(),
		&issue_run.worktree.branch_name,
		&github_token,
		project.github().command_path(),
	)?;
	dispatch_policy::detach_worktree_head_from_branch_if_checked_out(
		&issue_run.worktree.path,
		&issue_run.worktree.branch_name,
	)?;
	dispatch_policy::delete_local_branch_if_present(
		project.repo_root(),
		&issue_run.worktree.branch_name,
	)?;

	worktree_manager.remove_worktree_path_with_hooks(
		&issue_run.issue.identifier,
		&issue_run.worktree.branch_name,
		&issue_run.worktree.path,
		workflow.frontmatter().execution().workspace_hooks(),
	)?;
	state_store.clear_worktree(&issue_run.issue.id)?;

	Ok(())
}
