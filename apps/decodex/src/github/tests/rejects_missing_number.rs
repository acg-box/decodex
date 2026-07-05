use crate::github;

#[test]
fn rejects_missing_number() {
	let error = github::parse_pull_request_url("https://github.com/hack-ink/decodex/pull/")
		.expect_err("missing pull number should be rejected");

	assert!(error.to_string().contains("missing the pull request number"));
}
