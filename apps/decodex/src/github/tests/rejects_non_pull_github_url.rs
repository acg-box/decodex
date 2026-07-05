use crate::github;

#[test]
fn rejects_non_pull_github_url() {
	let error = github::parse_pull_request_url("https://github.com/hack-ink/decodex/issues/20")
		.expect_err("issue URL should be rejected");

	assert!(error.to_string().contains("/pull/<number>"));
}
