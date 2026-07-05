use crate::github::{self, RepositoryContext};

#[test]
fn repository_match_accepts_case_insensitive_pull_request_url() {
	let repository = RepositoryContext {
		owner: String::from("hack-ink"),
		name: String::from("decodex"),
		default_branch: String::from("main"),
		merge_commit_allowed: true,
	};

	assert!(
		github::pull_request_matches_repository(
			"https://github.com/Hack-Ink/Decodex/pull/9",
			&repository
		)
		.expect("same repository with different casing should parse")
	);
}
