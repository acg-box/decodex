//! GitHub pull-request readback helpers for recovery flows.

use crate::{
	github,
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
	recovery::context::RecoveryContext,
};

pub(super) fn inspect_rebind_pull_request(
	context: &RecoveryContext,
	pr_url: &str,
) -> Result<PullRequestLandingState> {
	let (landing_state, default_branch) = inspect_project_pull_request(context, pr_url)?;

	if landing_state.base_ref_name != default_branch {
		eyre::bail!(
			"Pull request `{}` targets `{}`, but configured default branch is `{}`.",
			pr_url,
			landing_state.base_ref_name,
			default_branch
		);
	}
	if landing_state.state != "OPEN" {
		eyre::bail!(
			"Pull request `{pr_url}` is `{}`; rebind requires `OPEN`.",
			landing_state.state
		);
	}
	if landing_state.is_draft {
		eyre::bail!("Pull request `{pr_url}` is still draft.");
	}

	Ok(landing_state)
}

pub(super) fn inspect_project_pull_request(
	context: &RecoveryContext,
	pr_url: &str,
) -> Result<(PullRequestLandingState, String)> {
	let github_token = context.config.github().resolve_token()?;
	let repository = github::inspect_repository_context(
		context.config.repo_root(),
		&github_token,
		context.config.github().command_path(),
	)?;

	if !github::pull_request_matches_repository(pr_url, &repository)? {
		eyre::bail!(
			"Pull request `{}` does not belong to configured repository `{}/{}`.",
			pr_url,
			repository.owner,
			repository.name
		);
	}

	let landing_state = github::inspect_pull_request_landing_state(
		context.config.repo_root(),
		pr_url,
		&github_token,
		context.config.github().command_path(),
	)?;

	Ok((landing_state, repository.default_branch))
}

pub(super) fn inspect_project_pull_request_merge_commit(
	context: &RecoveryContext,
	pr_url: &str,
) -> Result<String> {
	let github_token = context.config.github().resolve_token()?;

	github::inspect_pull_request_merge_commit(
		context.config.repo_root(),
		pr_url,
		&github_token,
		context.config.github().command_path(),
	)
}

pub(super) fn landing_url(landing_state: &PullRequestLandingState) -> &str {
	&landing_state.url
}
