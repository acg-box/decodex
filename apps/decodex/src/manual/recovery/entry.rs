use crate::{
	github,
	manual::{
		MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT, ManualLandContext, ManualLandRecoveryOutcome,
		ManualLandRequest, recovery::state,
	},
	prelude::{Result, eyre},
};

pub(in crate::manual) fn finalize_already_merged_manual_land_recovery(
	context: &ManualLandContext,
	request: &ManualLandRequest,
) -> Result<Option<ManualLandRecoveryOutcome>> {
	if !request.manual_authority || request.pr_url.is_none() {
		return Ok(None);
	}
	if !state::current_checkout_is_repo_root_default_branch(context)? {
		return Ok(None);
	}

	let landing_state = github::inspect_pull_request_landing_state(
		&context.canonical_repo_root,
		&context.pr_url,
		&context.github_token,
		context.github_command_path.as_deref(),
	)?;

	if landing_state.state != "MERGED" {
		eyre::bail!(
			"`decodex land --manual-authority --pr` can recover from the repo-root default branch only after the PR is `MERGED`; `{}` is `{}`.",
			context.pr_url,
			landing_state.state
		);
	}

	let merge_commit = github::wait_for_pull_request_merge_commit(
		&context.canonical_repo_root,
		&context.pr_url,
		&context.github_token,
		MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT,
		context.github_command_path.as_deref(),
	)?;

	state::ensure_already_merged_manual_land_recovery_ready(
		context,
		&landing_state,
		&merge_commit,
	)?;

	Ok(Some(ManualLandRecoveryOutcome { merge_commit }))
}
