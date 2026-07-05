use crate::{github, prelude::eyre};

#[test]
fn merge_commit_wait_retries_only_visibility_errors() {
	assert!(github::merge_commit_wait_error_is_retryable(&eyre::eyre!(
		"Pull request `https://github.com/hack-ink/decodex/pull/1` does not expose a merge commit after merge."
	)));
	assert!(!github::merge_commit_wait_error_is_retryable(&eyre::eyre!(
		"Failed to inspect merge result for `https://github.com/hack-ink/decodex/pull/1`: HTTP 401"
	)));
}
