use crate::prelude::{Result, eyre};

#[derive(Debug)]
pub(crate) struct PullRequestLocator {
	pub(crate) owner: String,
	pub(crate) repo: String,
	pub(crate) number: u64,
}

pub(crate) fn parse_pull_request_url(pr_url: &str) -> Result<PullRequestLocator> {
	let normalized = pr_url.trim().trim_end_matches('/');
	let suffix = normalized.strip_prefix("https://github.com/").ok_or_else(|| {
		eyre::eyre!("Pull request URL `{pr_url}` must start with `https://github.com/`.")
	})?;
	let mut segments = suffix.split('/');
	let owner = segments
		.next()
		.filter(|value| !value.is_empty())
		.ok_or_else(|| eyre::eyre!("Pull request URL `{pr_url}` is missing the owner."))?;
	let repo = segments
		.next()
		.filter(|value| !value.is_empty())
		.ok_or_else(|| eyre::eyre!("Pull request URL `{pr_url}` is missing the repository."))?;
	let pull_segment = segments
		.next()
		.ok_or_else(|| eyre::eyre!("Pull request URL `{pr_url}` is missing the `pull` segment."))?;

	if pull_segment != "pull" {
		eyre::bail!(
			"Pull request URL `{pr_url}` must use `/pull/<number>`, not `/{pull_segment}`."
		);
	}

	let number = segments
		.next()
		.ok_or_else(|| {
			eyre::eyre!("Pull request URL `{pr_url}` is missing the pull request number.")
		})?
		.parse::<u64>()
		.map_err(|error| {
			eyre::eyre!("Pull request URL `{pr_url}` has an invalid number: {error}")
		})?;

	Ok(PullRequestLocator { owner: owner.to_owned(), repo: repo.to_owned(), number })
}
