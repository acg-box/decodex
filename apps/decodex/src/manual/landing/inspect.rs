use std::{path::Path, thread};

use crate::{
	github,
	manual::{MANUAL_LAND_MERGEABILITY_RETRY_ATTEMPTS, MANUAL_LAND_MERGEABILITY_RETRY_DELAY},
	prelude::{Result, eyre},
	pull_request::{self, PullRequestLandingState},
};

pub(in crate::manual) fn inspect_pull_request_landing_state_for_manual_land(
	cwd: &Path,
	pr_url: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
	required_status_contexts: &[String],
	allowed_status_creators: &[String],
) -> Result<PullRequestLandingState> {
	let mut last_landing_state = None;

	for attempt in 1..=MANUAL_LAND_MERGEABILITY_RETRY_ATTEMPTS {
		let landing_state = github::inspect_pull_request_landing_state(
			cwd,
			pr_url,
			github_token,
			gh_command_path,
			required_status_contexts,
			allowed_status_creators,
		)?;

		if landing_state.state == "MERGED"
			|| !pull_request::mergeability_unknown(landing_state.gate_view())
		{
			return Ok(landing_state);
		}

		last_landing_state = Some(landing_state);

		if attempt < MANUAL_LAND_MERGEABILITY_RETRY_ATTEMPTS {
			tracing::info!(
				pr_url = %pr_url,
				attempt,
				mergeable = "UNKNOWN",
				merge_state_status = "UNKNOWN",
				"Pull request mergeability is unresolved; waiting for GitHub to recompute before validating manual land gates."
			);

			thread::sleep(MANUAL_LAND_MERGEABILITY_RETRY_DELAY);
		}
	}

	last_landing_state
		.ok_or_else(|| eyre::eyre!("Pull request `{pr_url}` landing state was unavailable."))
}
