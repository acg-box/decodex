use std::thread;

use reqwest::Error;
use reqwest::{
	Error, StatusCode,
	blocking::Client,
	header::{ACCEPT, AUTHORIZATION, HeaderMap, LINK, USER_AGENT},
};
use serde_json::{self, Value};

use crate::{
	GITHUB_REQUEST_ATTEMPTS, GITHUB_REQUEST_BACKOFF, GITHUB_REQUEST_TIMEOUT,
	RETRYABLE_GITHUB_STATUS_CODES,
	prelude::eyre::{self, Report},
};

#[derive(Debug)]
pub(super) struct GitHubApi {
	client: Client,
	token: Option<String>,
}
impl GitHubApi {
	pub(super) fn new(token: Option<String>) -> crate::prelude::Result<Self> {
		Ok(Self { client: Client::builder().timeout(GITHUB_REQUEST_TIMEOUT).build()?, token })
	}

	pub(super) fn get(&self, url: &str) -> crate::prelude::Result<GitHubResponse> {
		for attempt in 1..=GITHUB_REQUEST_ATTEMPTS {
			match self.try_get(url) {
				Ok(response) => return Ok(response),
				Err(error) if attempt < GITHUB_REQUEST_ATTEMPTS && error.is_retryable() => {
					thread::sleep(GITHUB_REQUEST_BACKOFF * attempt as u32);
				},
				Err(error) => return Err(error.into_report(url)),
			}
		}

		eyre::bail!("GitHub API request failed for {url}: exhausted retry loop")
	}

	pub(super) fn get_paginated(&self, url: &str) -> crate::prelude::Result<Vec<Value>> {
		let mut items = Vec::new();
		let mut next_url = Some(url.to_owned());

		while let Some(url) = next_url {
			let response = self.get(&url)?;
			let Some(page_items) = response.payload.as_array() else {
				eyre::bail!("Expected list payload from {url}");
			};

			items.extend(page_items.iter().cloned());

			next_url = response.next_url;
		}

		Ok(items)
	}

	fn try_get(&self, url: &str) -> std::result::Result<GitHubResponse, GitHubError> {
		let mut request = self
			.client
			.get(url)
			.header(ACCEPT, "application/vnd.github+json")
			.header(USER_AGENT, "decodex-radar");

		if let Some(token) = &self.token {
			request = request.header(AUTHORIZATION, format!("Bearer {token}"));
		}

		let response = request.send().map_err(GitHubError::Transport)?;
		let status = response.status();
		let next_url = parse_next_link_headers(response.headers());

		if !status.is_success() {
			let body = response
				.text()
				.unwrap_or_else(|error| format!("failed to read response body: {error}"));

			return Err(GitHubError::Status { status, body });
		}

		let body = response.text().map_err(GitHubError::Transport)?;
		let payload = serde_json::from_str(&body).map_err(|error| GitHubError::Json {
			error: error.to_string(),
			body: crate::body_excerpt(&body),
		})?;

		Ok(GitHubResponse { payload, next_url })
	}
}

#[derive(Debug)]
pub(super) struct GitHubResponse {
	pub(super) payload: Value,
	next_url: Option<String>,
}

#[derive(Debug)]
enum GitHubError {
	Status { status: StatusCode, body: String },
	Transport(Error),
	Json { error: String, body: String },
}
impl GitHubError {
	fn is_retryable(&self) -> bool {
		match self {
			Self::Status { status, .. } => RETRYABLE_GITHUB_STATUS_CODES.contains(status),
			Self::Transport(error) => error.is_timeout() || error.is_connect(),
			Self::Json { .. } => false,
		}
	}

	fn into_report(self, url: &str) -> Report {
		match self {
			Self::Status { status, body } => {
				eyre::eyre!("GitHub API request failed for {url}: {} {body}", status.as_u16())
			},
			Self::Transport(error) => eyre::eyre!("GitHub API request failed for {url}: {error}"),
			Self::Json { error, body } => {
				eyre::eyre!(
					"GitHub API response from {url} was not valid JSON: {error}; body: {body}"
				)
			},
		}
	}
}

fn parse_next_link_headers(headers: &HeaderMap) -> Option<String> {
	let header = headers.get(LINK)?.to_str().ok()?;

	header.split(',').find_map(|part| {
		let mut sections = part.trim().split(';');
		let url = sections.next()?.trim();
		let has_next = sections.any(|section| section.trim() == r#"rel="next""#);

		if has_next && url.starts_with('<') && url.ends_with('>') {
			Some(url[1..url.len() - 1].to_owned())
		} else {
			None
		}
	})
}
