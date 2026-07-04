use std::{
	path::Path,
	thread,
	time::{Duration, Instant},
};

use color_eyre::Report;

use crate::{
	github::merge_readback::{commit, readback},
	prelude::Result,
};

pub(crate) fn wait_for_pull_request_merge_commit(
	cwd: &Path,
	pr_url: &str,
	github_token: &str,
	timeout: Duration,
	gh_command_path: Option<&Path>,
) -> Result<String> {
	let deadline = Instant::now() + timeout;

	loop {
		match readback::inspect_pull_request_merge_commit(
			cwd,
			pr_url,
			github_token,
			gh_command_path,
		) {
			Ok(merge_commit) => return Ok(merge_commit),
			Err(error) if Instant::now() >= deadline => return Err(error),
			Err(error) if merge_commit_wait_error_is_retryable(&error) => {},
			Err(error) => return Err(error),
		};

		thread::sleep(Duration::from_secs(1));
	}
}

pub(crate) fn wait_for_commit_subject(
	cwd: &Path,
	pr_url: &str,
	commit_oid: &str,
	github_token: &str,
	timeout: Duration,
	gh_command_path: Option<&Path>,
) -> Result<String> {
	let deadline = Instant::now() + timeout;

	loop {
		match commit::inspect_commit_subject(cwd, pr_url, commit_oid, github_token, gh_command_path)
		{
			Ok(subject) => return Ok(subject),
			Err(error) if Instant::now() >= deadline => return Err(error),
			Err(error) if commit_subject_wait_error_is_retryable(&error) => {},
			Err(error) => return Err(error),
		};

		thread::sleep(Duration::from_secs(1));
	}
}

pub(crate) fn merge_commit_wait_error_is_retryable(error: &Report) -> bool {
	let message = error.to_string();

	message.contains("did not reach `MERGED` state after landing")
		|| message.contains("does not expose a merge commit after merge")
}

pub(crate) fn commit_subject_wait_error_is_retryable(error: &Report) -> bool {
	let message = error.to_string().to_ascii_lowercase();

	message.contains("failed to inspect merge commit")
		&& (message.contains("not found") || message.contains("http 404"))
}
