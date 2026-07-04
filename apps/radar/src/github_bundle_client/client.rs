use std::time::Duration;

use reqwest::blocking::Client;
use serde_json::Value;

use crate::{
	github_bundle_client::request::GITHUB_REQUEST_TIMEOUT_SECONDS,
	prelude::{Result, eyre},
};

pub(crate) struct GithubClient {
	pub(in crate::github_bundle_client) http: Client,
	pub(in crate::github_bundle_client) token: Option<String>,
}
impl GithubClient {
	pub(crate) fn new(token: Option<&str>) -> Result<Self> {
		Ok(Self {
			http: Client::builder()
				.timeout(Duration::from_secs(GITHUB_REQUEST_TIMEOUT_SECONDS))
				.build()?,
			token: token.map(str::to_owned),
		})
	}

	pub(crate) fn build_pr_bundle(
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

	pub(crate) fn build_commit_bundle(
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

	pub(crate) fn maybe_promote_commit_to_pr(&self, repo: &str, commit_sha: &str) -> Option<u64> {
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
}
