use crate::{github, prelude::eyre};

#[test]
fn commit_subject_wait_retries_only_not_found_visibility_errors() {
	assert!(github::commit_subject_wait_error_is_retryable(&eyre::eyre!(
		"Failed to inspect merge commit `abc` for `https://github.com/hack-ink/decodex/pull/1`: HTTP 404 Not Found"
	)));
	assert!(!github::commit_subject_wait_error_is_retryable(&eyre::eyre!(
		"Failed to inspect merge commit `abc` for `https://github.com/hack-ink/decodex/pull/1`: HTTP 401 Unauthorized"
	)));
}
