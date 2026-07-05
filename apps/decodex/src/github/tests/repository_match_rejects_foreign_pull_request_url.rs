use crate::github::{self, RepositoryContext};

#[test]
fn repository_match_rejects_foreign_pull_request_url() {
	let repository = RepositoryContext {
		owner: String::from("hack-ink"),
		name: String::from("decodex"),
		default_branch: String::from("main"),
		merge_commit_allowed: true,
	};

	assert!(
		!github::pull_request_matches_repository(
			"https://github.com/other-org/other-repo/pull/9",
			&repository
		)
		.expect("foreign pull request URL should parse")
	);
}
