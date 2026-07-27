use std::{collections::HashSet, thread};

use reqwest::{
	Error, StatusCode, Url,
	blocking::Client,
	header::{ACCEPT, AUTHORIZATION, HeaderMap, LINK, RETRY_AFTER, USER_AGENT},
};
use serde_json::{self, Value};

use crate::{
	GITHUB_REQUEST_ATTEMPTS, GITHUB_REQUEST_BACKOFF, GITHUB_REQUEST_TIMEOUT,
	RETRYABLE_GITHUB_STATUS_CODES,
	prelude::eyre::{self, Report},
};

const GITHUB_API_ORIGIN: &str = "https://api.github.com/";
const MAX_GITHUB_PAGES: usize = 100;
const MAX_GITHUB_PAGINATED_ITEMS: usize = 10_000;

#[derive(Debug)]
pub(crate) struct GitHubApi {
	client: Client,
	origin: Url,
	token: Option<String>,
}
impl GitHubApi {
	pub(super) fn new(token: Option<String>) -> crate::prelude::Result<Self> {
		Self::new_with_origin(token, Url::parse(GITHUB_API_ORIGIN)?)
	}

	fn new_with_origin(token: Option<String>, origin: Url) -> crate::prelude::Result<Self> {
		Ok(Self {
			client: Client::builder()
				.timeout(GITHUB_REQUEST_TIMEOUT)
				.redirect(reqwest::redirect::Policy::none())
				.build()?,
			origin,
			token,
		})
	}

	#[cfg(test)]
	pub(crate) fn new_for_test(
		token: Option<String>,
		origin: &str,
	) -> crate::prelude::Result<Self> {
		Self::new_with_origin(token, Url::parse(origin)?)
	}

	pub(super) fn get(&self, url: &str) -> crate::prelude::Result<GitHubResponse> {
		let url = self.validated_url(url)?;

		for attempt in 1..=GITHUB_REQUEST_ATTEMPTS {
			match self.try_get(&url) {
				Ok(response) => return Ok(response),
				Err(error) if attempt < GITHUB_REQUEST_ATTEMPTS && error.is_retryable() => {
					thread::sleep(GITHUB_REQUEST_BACKOFF * attempt as u32);
				},
				Err(error) => return Err(error.into_report(url.as_str())),
			}
		}

		eyre::bail!("GitHub API request failed for {url}: exhausted retry loop")
	}

	pub(super) fn get_paginated(&self, url: &str) -> crate::prelude::Result<Vec<Value>> {
		self.get_paginated_bounded(url, MAX_GITHUB_PAGES, MAX_GITHUB_PAGINATED_ITEMS)
	}

	fn get_paginated_bounded(
		&self,
		url: &str,
		max_pages: usize,
		max_items: usize,
	) -> crate::prelude::Result<Vec<Value>> {
		let mut items = Vec::new();
		let mut next_url = Some(url.to_owned());
		let mut visited = HashSet::new();
		let mut pages = 0_usize;

		while let Some(url) = next_url {
			let validated = self.validated_url(&url)?;
			let canonical = validated.as_str().to_owned();

			if !visited.insert(canonical.clone()) {
				eyre::bail!("GitHub API pagination cycle detected at {canonical}");
			}
			if pages >= max_pages {
				eyre::bail!("GitHub API pagination exceeds the {max_pages}-page limit");
			}
			pages += 1;

			let response = self.get(validated.as_str())?;
			let Some(page_items) = response.payload.as_array() else {
				eyre::bail!("Expected list payload from {url}");
			};
			if page_items.len() > max_items.saturating_sub(items.len()) {
				eyre::bail!("GitHub API pagination exceeds the {max_items}-item limit");
			}

			items.extend(page_items.iter().cloned());

			next_url = response.next_url;
		}

		Ok(items)
	}

	#[cfg(test)]
	pub(crate) fn get_paginated_for_test(
		&self,
		url: &str,
		max_pages: usize,
		max_items: usize,
	) -> crate::prelude::Result<Vec<Value>> {
		self.get_paginated_bounded(url, max_pages, max_items)
	}

	fn validated_url(&self, url: &str) -> crate::prelude::Result<Url> {
		let parsed =
			Url::parse(url).map_err(|error| eyre::eyre!("GitHub API URL is invalid: {error}"))?;

		if !parsed.username().is_empty() || parsed.password().is_some() {
			eyre::bail!("GitHub API URL must not contain user information");
		}
		if parsed.origin() != self.origin.origin() {
			eyre::bail!(
				"GitHub API URL must stay on the pinned origin {}",
				self.origin.origin().ascii_serialization()
			);
		}

		Ok(parsed)
	}

	fn try_get(&self, url: &Url) -> std::result::Result<GitHubResponse, GitHubError> {
		let mut request = self
			.client
			.get(url.clone())
			.header(ACCEPT, "application/vnd.github+json")
			.header(USER_AGENT, "decodex-radar");

		if let Some(token) = &self.token {
			request = request.header(AUTHORIZATION, format!("Bearer {token}"));
		}

		let response = request.send().map_err(GitHubError::Transport)?;
		let status = response.status();
		let headers = response.headers().clone();
		let next_url = parse_next_link_headers(&headers);

		if !status.is_success() {
			let body = response
				.text()
				.unwrap_or_else(|error| format!("failed to read response body: {error}"));

			if let Some(rate_limit) = GitHubRateLimit::from_response(status, &headers, &body) {
				return Err(GitHubError::RateLimit(rate_limit));
			}

			return Err(GitHubError::Status { status, body: crate::body_excerpt(&body) });
		}

		let body = response.text().map_err(GitHubError::Transport)?;
		let payload = serde_json::from_str(&body).map_err(|error| GitHubError::Json {
			error: error.to_string(),
			body: crate::body_excerpt(&body),
		})?;

		Ok(GitHubResponse { payload, headers, next_url })
	}
}

#[derive(Debug)]
pub(crate) struct GitHubResponse {
	pub(crate) payload: Value,
	pub(crate) headers: HeaderMap,
	next_url: Option<String>,
}

#[derive(Debug)]
enum GitHubError {
	RateLimit(GitHubRateLimit),
	Status { status: StatusCode, body: String },
	Transport(Error),
	Json { error: String, body: String },
}
impl GitHubError {
	fn is_retryable(&self) -> bool {
		match self {
			Self::RateLimit(_) => false,
			Self::Status { status, .. } => RETRYABLE_GITHUB_STATUS_CODES.contains(status),
			Self::Transport(error) =>
				error.is_timeout() || error.is_connect() || error.is_body() || error.is_decode(),
			Self::Json { .. } => true,
		}
	}

	fn into_report(self, url: &str) -> Report {
		match self {
			Self::RateLimit(rate_limit) => eyre::eyre!(
				"GitHub API rate limit exceeded for {url}: reason_code=github_rate_limited status={} remaining={} reset_epoch={} retry_after={} message={}",
				rate_limit.status.as_u16(),
				rate_limit.remaining.as_deref().unwrap_or("unknown"),
				rate_limit.reset_epoch.as_deref().unwrap_or("unknown"),
				rate_limit.retry_after.as_deref().unwrap_or("unknown"),
				rate_limit.message,
			),
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

#[derive(Debug)]
struct GitHubRateLimit {
	status: StatusCode,
	remaining: Option<String>,
	reset_epoch: Option<String>,
	retry_after: Option<String>,
	message: String,
}
impl GitHubRateLimit {
	fn from_response(status: StatusCode, headers: &HeaderMap, body: &str) -> Option<Self> {
		let remaining = header_string(headers, "x-ratelimit-remaining");
		let reset_epoch = header_string(headers, "x-ratelimit-reset");
		let retry_after =
			headers.get(RETRY_AFTER).and_then(|value| value.to_str().ok()).map(crate::body_excerpt);
		let message = response_message(body);
		let is_rate_limit = status == StatusCode::TOO_MANY_REQUESTS
			|| status == StatusCode::FORBIDDEN
				&& (remaining.as_deref() == Some("0")
					|| retry_after.is_some()
					|| message.to_ascii_lowercase().contains("rate limit"));

		is_rate_limit.then_some(Self { status, remaining, reset_epoch, retry_after, message })
	}
}

fn header_string(headers: &HeaderMap, name: &str) -> Option<String> {
	headers.get(name).and_then(|value| value.to_str().ok()).map(crate::body_excerpt)
}

fn response_message(body: &str) -> String {
	serde_json::from_str::<Value>(body)
		.ok()
		.and_then(|payload| payload.get("message").and_then(Value::as_str).map(str::to_owned))
		.unwrap_or_else(|| crate::body_excerpt(body))
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
