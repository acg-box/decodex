use std::path::Path;

use crate::{
	agent::{ReviewExecutionMode, ReviewHandoffContext},
	config::ServiceConfig,
	orchestrator::{self, IssueDispatchMode, IssueRunPlan},
	prelude::{Result, eyre},
	state::{ReviewLifecycleRecord, StateStore},
};

pub(crate) fn build_review_run_context(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<ReviewHandoffContext> {
	match issue_run.dispatch_mode {
		IssueDispatchMode::ReviewRepair => {
			orchestrator::validate_review_repair_runtime(project, false)?;

			let lifecycle_record = read_retained_review_lifecycle(project, state_store, issue_run)?
				.ok_or_else(|| {
					eyre::eyre!(
						"Retained review-repair run `{}` for issue `{}` requires an existing review lifecycle authority.",
						issue_run.run_id,
						issue_run.issue.identifier
					)
				})?;

			Ok(ReviewHandoffContext {
				attempt_number: issue_run.attempt_number,
				branch_name: issue_run.worktree.branch_name.clone(),
				run_id: issue_run.run_id.clone(),
				service_id: project.service_id().to_owned(),
				worktree_path: orchestrator::relative_worktree_path(project, &issue_run.worktree),
				cwd: issue_run.worktree.path.clone(),
				github_token_env_var: Some(project.github().token_env_var().to_owned()),
				github_command_path: project.github().command_path().map(Path::to_path_buf),
				review_level: project.codex().review_level(),
				mode: ReviewExecutionMode::Repair,
				recorded_pr_url: Some(lifecycle_record.pr_url().to_owned()),
			})
		},
		IssueDispatchMode::Closeout => {
			orchestrator::validate_closeout_runtime(project, false)?;

			let lifecycle_record = read_retained_review_lifecycle(project, state_store, issue_run)?
				.ok_or_else(|| {
					eyre::eyre!(
						"Retained closeout run `{}` for issue `{}` requires an existing review lifecycle authority.",
						issue_run.run_id,
						issue_run.issue.identifier
					)
				})?;

			Ok(ReviewHandoffContext {
				attempt_number: issue_run.attempt_number,
				branch_name: issue_run.worktree.branch_name.clone(),
				run_id: issue_run.run_id.clone(),
				service_id: project.service_id().to_owned(),
				worktree_path: orchestrator::relative_worktree_path(project, &issue_run.worktree),
				cwd: issue_run.worktree.path.clone(),
				github_token_env_var: Some(project.github().token_env_var().to_owned()),
				github_command_path: project.github().command_path().map(Path::to_path_buf),
				review_level: project.codex().review_level(),
				mode: ReviewExecutionMode::Closeout,
				recorded_pr_url: Some(lifecycle_record.pr_url().to_owned()),
			})
		},
		_ => Ok(ReviewHandoffContext {
			attempt_number: issue_run.attempt_number,
			branch_name: issue_run.worktree.branch_name.clone(),
			run_id: issue_run.run_id.clone(),
			service_id: project.service_id().to_owned(),
			worktree_path: orchestrator::relative_worktree_path(project, &issue_run.worktree),
			cwd: issue_run.worktree.path.clone(),
			github_token_env_var: Some(project.github().token_env_var().to_owned()),
			github_command_path: project.github().command_path().map(Path::to_path_buf),
			review_level: project.codex().review_level(),
			mode: ReviewExecutionMode::Handoff,
			recorded_pr_url: None,
		}),
	}
}

fn read_retained_review_lifecycle(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<Option<ReviewLifecycleRecord>> {
	state_store.review_lifecycle_record(
		project.service_id(),
		&issue_run.issue.id,
		&issue_run.worktree.branch_name,
	)
}
