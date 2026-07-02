use serde_json::Value;

use crate::{
	GitHubApi,
	prelude::{Result, eyre},
};

pub(crate) struct RecentCommit {
	pub(crate) sha: String,
	pub(crate) title: String,
	pub(crate) url: String,
	pub(crate) committed_at: Option<String>,
}

pub(super) fn recent_commits(
	api: &GitHubApi,
	repo: &str,
	search_limit: usize,
) -> Result<(String, Vec<RecentCommit>)> {
	let default_branch = crate::repo_default_branch(api, repo)?;
	let url = format!(
		"https://api.github.com/repos/{repo}/commits?sha={}&per_page={search_limit}",
		crate::percent_encode(&default_branch)
	);
	let payload = api.get(&url)?.payload;
	let Some(items) = payload.as_array() else {
		eyre::bail!("Expected commits list payload from GitHub API");
	};
	let commits = items.iter().filter_map(recent_commit_from_value).collect::<Vec<_>>();

	Ok((default_branch, commits))
}

pub(super) fn maybe_promote_commit_to_pr(
	api: &GitHubApi,
	repo: &str,
	commit_sha: &str,
) -> Result<Option<u64>> {
	let url = format!("https://api.github.com/repos/{repo}/commits/{commit_sha}/pulls");
	let pulls = match api.get_paginated(&url) {
		Ok(pulls) => pulls,
		Err(_) => return Ok(None),
	};

	Ok(pulls.first().and_then(|first| first.get("number")).and_then(Value::as_u64))
}

fn recent_commit_from_value(item: &Value) -> Option<RecentCommit> {
	let commit = item.get("commit")?.as_object()?;
	let sha = item.get("sha")?.as_str()?.to_owned();
	let url = item.get("html_url")?.as_str()?.to_owned();
	let message = commit.get("message")?.as_str()?;

	if message.is_empty() {
		return None;
	}

	Some(RecentCommit {
		sha,
		title: crate::first_line(message),
		url,
		committed_at: commit
			.get("committer")
			.and_then(Value::as_object)
			.and_then(|committer| committer.get("date"))
			.and_then(Value::as_str)
			.map(str::to_owned),
	})
}
