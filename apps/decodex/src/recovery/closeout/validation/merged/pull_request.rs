use std::{path::Path, process::Command};

use crate::{
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
	recovery::{context::RecoveryContext, pull_request_inspection},
};

pub(in crate::recovery) fn validate_merged_closeout_pull_request(
	context: &RecoveryContext,
	landing_state: &PullRequestLandingState,
	default_branch: &str,
) -> Result<()> {
	if landing_state.base_ref_name != default_branch {
		eyre::bail!(
			"Pull request `{}` targets `{}`, but configured default branch is `{default_branch}`.",
			pull_request_inspection::landing_url(landing_state),
			landing_state.base_ref_name
		);
	}
	if landing_state.state != "MERGED" {
		eyre::bail!(
			"Pull request `{}` is `{}`; merged closeout recovery requires `MERGED`.",
			pull_request_inspection::landing_url(landing_state),
			landing_state.state
		);
	}
	if landing_state.head_ref_name.trim().is_empty() {
		eyre::bail!(
			"Pull request `{}` does not expose the merged head branch required for retained lane reconciliation.",
			pull_request_inspection::landing_url(landing_state)
		);
	}
	if landing_state.head_ref_name == default_branch {
		eyre::bail!(
			"Pull request `{}` uses default branch `{default_branch}` as its head; merged closeout recovery cannot prove retained lane identity.",
			pull_request_inspection::landing_url(landing_state)
		);
	}

	let remote_ref = format!("refs/remotes/origin/{default_branch}");
	let output = Command::new("git")
		.arg("-C")
		.arg(context.config.repo_root())
		.args(["rev-parse", "--verify", remote_ref.as_str()])
		.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Configured repo root `{}` does not expose `{remote_ref}`; sync the default branch before merged closeout recovery: {}",
			context.config.repo_root().display(),
			stderr.trim()
		);
	}

	Ok(())
}

pub(in crate::recovery) fn ensure_merge_commit_reachable_from_remote_default_branch(
	repo_root: &Path,
	pr_url: &str,
	merge_commit: &str,
	default_branch: &str,
) -> Result<()> {
	let remote_ref = format!("refs/remotes/origin/{default_branch}");
	let status = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["merge-base", "--is-ancestor", merge_commit, remote_ref.as_str()])
		.status()?;

	if status.success() {
		return Ok(());
	}
	if status.code() == Some(1) {
		eyre::bail!(
			"Configured repo root `{}` remote `{remote_ref}` does not contain merge commit `{merge_commit}` for `{pr_url}`.",
			repo_root.display()
		);
	}

	eyre::bail!(
		"`git merge-base --is-ancestor {merge_commit} {remote_ref}` failed in `{}` with status `{status}`.",
		repo_root.display()
	)
}
