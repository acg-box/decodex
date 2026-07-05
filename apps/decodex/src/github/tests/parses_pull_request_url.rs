use crate::github;

#[test]
fn parses_pull_request_url() {
	let locator = github::parse_pull_request_url("https://github.com/hack-ink/decodex/pull/20")
		.expect("pull request URL should parse");

	assert_eq!(locator.owner, "hack-ink");
	assert_eq!(locator.repo, "decodex");
	assert_eq!(locator.number, 20);
}
