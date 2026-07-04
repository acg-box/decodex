use std::process::Command;

use crate::orchestrator::execution::{
	self, GitCredentialSource, IssueRunPlan, Report, Result, RetainedReviewRepairPushFailed,
	RetainedReviewRepairPushFailureKind, ServiceConfig,
};

pub(crate) fn push_retained_review_repair_head(
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
	pr_url: Option<&str>,
) -> Result<()> {
	let token_env_var = project.github().token_env_var();
	let github_token =
		execution::resolve_configured_env_var("github.token_env_var", Some(token_env_var))
			.map_err(|error| {
				Report::new(RetainedReviewRepairPushFailed {
					issue_identifier: issue_run.issue.identifier.clone(),
					run_id: issue_run.run_id.clone(),
					branch_name: issue_run.worktree.branch_name.clone(),
					pr_url: pr_url.map(ToOwned::to_owned),
					kind: RetainedReviewRepairPushFailureKind::Auth,
					detail: error.to_string(),
				})
			})?;
	let git_credentials =
		GitCredentialSource::new(token_env_var, &github_token).materialize_github_credentials();
	let refspec = format!("HEAD:{}", issue_run.worktree.branch_name);
	let mut command = Command::new("git");

	command.arg("-C").arg(&issue_run.worktree.path).arg("push").arg("origin").arg(&refspec);
	git_credentials.apply_to(&mut command);

	let output = command.output().map_err(|error| {
		Report::new(RetainedReviewRepairPushFailed {
			issue_identifier: issue_run.issue.identifier.clone(),
			run_id: issue_run.run_id.clone(),
			branch_name: issue_run.worktree.branch_name.clone(),
			pr_url: pr_url.map(ToOwned::to_owned),
			kind: RetainedReviewRepairPushFailureKind::Failed,
			detail: error.to_string(),
		})
	})?;

	if output.status.success() {
		return Ok(());
	}

	let detail = execution::repo_gate_output_text(&output);
	let kind = self::classify_retained_review_repair_push_failure(&detail);

	Err(Report::new(RetainedReviewRepairPushFailed {
		issue_identifier: issue_run.issue.identifier.clone(),
		run_id: issue_run.run_id.clone(),
		branch_name: issue_run.worktree.branch_name.clone(),
		pr_url: pr_url.map(ToOwned::to_owned),
		kind,
		detail,
	}))
}

fn classify_retained_review_repair_push_failure(
	detail: &str,
) -> RetainedReviewRepairPushFailureKind {
	let normalized = detail.to_ascii_lowercase();

	if normalized.contains("authentication failed")
		|| normalized.contains("could not read username")
		|| normalized.contains("permission denied")
		|| normalized.contains("repository not found")
		|| normalized.contains("403")
		|| normalized.contains("401")
	{
		return RetainedReviewRepairPushFailureKind::Auth;
	}
	if normalized.contains("src refspec")
		|| normalized.contains("dst refspec")
		|| normalized.contains("invalid refspec")
	{
		return RetainedReviewRepairPushFailureKind::Refspec;
	}
	if normalized.contains("rejected")
		|| normalized.contains("non-fast-forward")
		|| normalized.contains("fetch first")
		|| normalized.contains("protected branch hook declined")
	{
		return RetainedReviewRepairPushFailureKind::RemoteRejected;
	}

	RetainedReviewRepairPushFailureKind::Failed
}
