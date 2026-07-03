use std::{thread, time::Duration};

use reqwest::{
	blocking::Client,
	header::{ACCEPT, HeaderMap, LINK, USER_AGENT},
};
use serde_json::{self, Value};

use crate::prelude::Result;
use crate::prelude::eyre;
use crate::{GITHUB_REQUEST_ATTEMPTS, RETRYABLE_GITHUB_STATUS_CODES};

const GITHUB_REQUEST_BACKOFF_SECONDS: u64 = 1;
const GITHUB_REQUEST_TIMEOUT_SECONDS: u64 = 30;

pub(super) struct GithubClient {
	http: Client,
	token: Option<String>,
}
impl GithubClient {
	pub(super) fn new(token: Option<&str>) -> Result<Self> {
		Ok(Self {
			http: Client::builder()
				.timeout(Duration::from_secs(GITHUB_REQUEST_TIMEOUT_SECONDS))
				.build()?,
			token: token.map(str::to_owned),
		})
	}

	pub(super) fn build_pr_bundle(
		&self,
		repo: &str,
		pr_number: u64,
		notes: &[String],
	) -> Result<Value> {
		let (pr, _) =
			self.github_request(&format!("https://api.github.com/repos/{repo}/pulls/{pr_number}"))?;
		let commits = self.github_paginated(&format!(
			"https://api.github.com/repos/{repo}/pulls/{pr_number}/commits?per_page=100"
		))?;
		let files = self.github_paginated(&format!(
			"https://api.github.com/repos/{repo}/pulls/{pr_number}/files?per_page=100"
		))?;
		let default_branch = self.repo_default_branch(repo)?;

		crate::build_pr_bundle_from_sources(repo, &pr, &commits, &files, &default_branch, notes)
	}

	pub(super) fn build_commit_bundle(
		&self,
		repo: &str,
		commit_sha: &str,
		notes: &[String],
	) -> Result<Value> {
		let (commit, _) = self
			.github_request(&format!("https://api.github.com/repos/{repo}/commits/{commit_sha}"))?;
		let default_branch = self.repo_default_branch(repo)?;

		crate::build_commit_bundle_from_sources(repo, &commit, &default_branch, notes)
	}

	pub(super) fn maybe_promote_commit_to_pr(&self, repo: &str, commit_sha: &str) -> Option<u64> {
		let pulls = self
			.github_paginated(&format!(
				"https://api.github.com/repos/{repo}/commits/{commit_sha}/pulls"
			))
			.ok()?;
		let first = pulls.first()?.as_object()?;

		first.get("number").and_then(Value::as_u64)
	}

	fn repo_default_branch(&self, repo: &str) -> Result<String> {
		let (payload, _) = self.github_request(&format!("https://api.github.com/repos/{repo}"))?;
		let default_branch = payload.get("default_branch").and_then(Value::as_str);

		default_branch
			.filter(|value| !value.is_empty())
			.map(str::to_owned)
			.ok_or_else(|| eyre::eyre!("Unable to resolve default branch for {repo}"))
	}

	fn github_paginated(&self, url: &str) -> Result<Vec<Value>> {
		let mut items = Vec::new();
		let mut next_url = Some(url.to_owned());

		while let Some(url) = next_url {
			let (payload, headers) = self.github_request(&url)?;
			let Some(values) = payload.as_array() else {
				eyre::bail!("Expected list payload from {url}");
			};

			items.extend(values.iter().cloned());

			next_url =
				headers.get(LINK).and_then(|value| value.to_str().ok()).and_then(parse_next_link);
		}

		Ok(items)
	}

	fn github_request(&self, url: &str) -> Result<(Value, HeaderMap)> {
		for attempt in 1..=GITHUB_REQUEST_ATTEMPTS {
			let mut request = self
				.http
				.get(url)
				.header(ACCEPT, "application/vnd.github+json")
				.header(USER_AGENT, "decodex-github-bundle-builder");

			if let Some(token) = &self.token {
				request = request.bearer_auth(token);
			}

			match request.send() {
				Ok(response) => {
					let status = response.status();
					let headers = response.headers().clone();

					if status.is_success() {
						let body = response.text()?;
						let payload = serde_json::from_str(&body).map_err(|error| {
							eyre::eyre!(
								"GitHub API response from {url} was not valid JSON: {error}; body: {}",
								crate::body_excerpt(&body)
							)
						})?;

						return Ok((payload, headers));
					}

					let details = response.text().unwrap_or_default();

					if RETRYABLE_GITHUB_STATUS_CODES.contains(&status)
						&& attempt < GITHUB_REQUEST_ATTEMPTS
					{
						sleep_before_retry(attempt);

						continue;
					}

					eyre::bail!("GitHub API request failed for {url}: {status} {details}");
				},
				Err(error) => {
					if !error.is_timeout() && !error.is_connect()
						|| attempt == GITHUB_REQUEST_ATTEMPTS
					{
						eyre::bail!("GitHub API request failed for {url}: {error}");
					}

					sleep_before_retry(attempt);
				},
			}
		}

		eyre::bail!("GitHub API request failed for {url}: exhausted retry loop")
	}
}

fn parse_next_link(header: &str) -> Option<String> {
	for part in header.split(',') {
		let mut sections = part.trim().split(';');
		let Some(url_part) = sections.next() else {
			continue;
		};

		if sections.any(|section| section.trim() == r#"rel="next""#) {
			return Some(url_part.trim().trim_start_matches('<').trim_end_matches('>').into());
		}
	}

	None
}

fn sleep_before_retry(attempt: usize) {
	thread::sleep(Duration::from_secs(GITHUB_REQUEST_BACKOFF_SECONDS * attempt as u64));
}
