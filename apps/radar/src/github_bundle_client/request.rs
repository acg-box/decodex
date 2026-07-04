use std::{thread, time::Duration};

use reqwest::header::{ACCEPT, HeaderMap, USER_AGENT};
use serde_json::{self, Value};

use crate::{
	GITHUB_REQUEST_ATTEMPTS, RETRYABLE_GITHUB_STATUS_CODES,
	github_bundle_client::GithubClient,
	prelude::{Result, eyre},
};

pub(in crate::github_bundle_client) const GITHUB_REQUEST_TIMEOUT_SECONDS: u64 = 30;

const GITHUB_REQUEST_BACKOFF_SECONDS: u64 = 1;

impl GithubClient {
	pub(in crate::github_bundle_client) fn github_request(
		&self,
		url: &str,
	) -> Result<(Value, HeaderMap)> {
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

fn sleep_before_retry(attempt: usize) {
	thread::sleep(Duration::from_secs(GITHUB_REQUEST_BACKOFF_SECONDS * attempt as u64));
}
