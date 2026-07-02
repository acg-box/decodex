use serde_json::Value;

use crate::{GitHubApi, prelude::Result};

#[derive(Clone, Debug)]
pub(super) struct BundleFile {
	pub(super) path: String,
	pub(super) patch_excerpt: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct BundleCommit {
	pub(super) sha: String,
	pub(super) message: String,
}

#[derive(Clone, Debug)]
pub(super) struct BundlePr {
	pub(super) number: u64,
	pub(super) title: String,
	pub(super) body: String,
	pub(super) state: String,
	pub(super) url: String,
}

#[derive(Clone, Debug)]
pub(super) struct SourceBundle {
	pub(super) primary_pr: Option<BundlePr>,
	pub(super) commits: Vec<BundleCommit>,
	pub(super) files: Vec<BundleFile>,
}

pub(super) fn build_pr_bundle(api: &GitHubApi, repo: &str, pr_number: u64) -> Result<SourceBundle> {
	let pr = api.get(&format!("https://api.github.com/repos/{repo}/pulls/{pr_number}"))?.payload;
	let commits = api.get_paginated(&format!(
		"https://api.github.com/repos/{repo}/pulls/{pr_number}/commits?per_page=100"
	))?;
	let files = api.get_paginated(&format!(
		"https://api.github.com/repos/{repo}/pulls/{pr_number}/files?per_page=100"
	))?;

	Ok(SourceBundle {
		primary_pr: Some(BundlePr {
			number: crate::required_value_u64(&pr, "number")?,
			title: crate::required_value_string(&pr, "title")?,
			body: crate::optional_value_string(&pr, "body").unwrap_or_default(),
			state: if crate::optional_value_string(&pr, "merged_at").is_some() {
				"merged".to_owned()
			} else {
				crate::required_value_string(&pr, "state")?
			},
			url: crate::required_value_string(&pr, "html_url")?,
		}),
		commits: commits.iter().filter_map(bundle_commit_from_pr_commit).collect(),
		files: files.iter().filter_map(bundle_file_from_value).collect(),
	})
}

pub(super) fn build_commit_bundle(
	api: &GitHubApi,
	repo: &str,
	commit_sha: &str,
) -> Result<SourceBundle> {
	let commit =
		api.get(&format!("https://api.github.com/repos/{repo}/commits/{commit_sha}"))?.payload;
	let files = commit.get("files").and_then(Value::as_array).cloned().unwrap_or_default();
	let message = commit.pointer("/commit/message").and_then(Value::as_str).unwrap_or_default();

	Ok(SourceBundle {
		primary_pr: None,
		commits: vec![BundleCommit {
			sha: crate::required_value_string(&commit, "sha")?,
			message: crate::first_line(message),
		}],
		files: files.iter().filter_map(bundle_file_from_value).collect(),
	})
}

fn bundle_commit_from_pr_commit(item: &Value) -> Option<BundleCommit> {
	Some(BundleCommit {
		sha: item.get("sha")?.as_str()?.to_owned(),
		message: crate::first_line(item.pointer("/commit/message")?.as_str()?),
	})
}

fn bundle_file_from_value(item: &Value) -> Option<BundleFile> {
	Some(BundleFile {
		path: item.get("filename")?.as_str()?.to_owned(),
		patch_excerpt: item.get("patch").and_then(Value::as_str).map(crate::truncate_patch_excerpt),
	})
}
