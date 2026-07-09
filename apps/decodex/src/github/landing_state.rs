mod merge;
mod model;
mod query;

use std::path::Path;

use crate::{
	github,
	github::landing_state::query::PullRequestLandingStatePageQuery,
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
};

pub(crate) fn inspect_pull_request_landing_state(
	cwd: &Path,
	pr_url: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
	required_status_contexts: &[String],
	allowed_status_creators: &[String],
) -> Result<PullRequestLandingState> {
	let locator = github::parse_pull_request_url(pr_url)?;
	let mut review_threads_after: Option<String> = None;
	let mut landing_state: Option<PullRequestLandingState> = None;

	loop {
		let pull_request =
			query::query_pull_request_landing_state_page(PullRequestLandingStatePageQuery {
				cwd,
				owner: &locator.owner,
				repo: &locator.repo,
				number: locator.number,
				review_threads_after: review_threads_after.as_deref(),
				pr_url,
				github_token,
				gh_command_path,
			})?;
		let next_cursor = match &mut landing_state {
			Some(landing_state) =>
				merge::merge_pull_request_landing_state_page(landing_state, &pull_request)?,
			None => {
				let next_cursor =
					merge::next_pull_request_review_threads_cursor(&pull_request, pr_url)?;

				landing_state = Some(merge::pull_request_landing_state_from_page(&pull_request));

				next_cursor
			},
		};
		let Some(next_cursor) = next_cursor else {
			break;
		};

		review_threads_after = Some(next_cursor);
	}

	let mut landing_state = landing_state.ok_or_else(|| {
		eyre::eyre!("GitHub GraphQL response for `{pr_url}` did not include a pull request.")
	})?;

	landing_state.required_status_contexts = github::inspect_required_commit_status_contexts(
		cwd,
		&locator.owner,
		&locator.repo,
		&landing_state.head_ref_oid,
		landing_state.base_ref_oid.as_deref(),
		required_status_contexts,
		allowed_status_creators,
		github_token,
		gh_command_path,
	)?;

	Ok(landing_state)
}
