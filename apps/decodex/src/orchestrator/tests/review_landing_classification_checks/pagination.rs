use crate::orchestrator::{
	self, PullRequestActor, PullRequestIssueCommentConnection, PullRequestIssueCommentNode,
	PullRequestIssueCommentsNode, PullRequestPageInfo, PullRequestRepository,
	PullRequestRepositoryOwner, PullRequestReviewStateNode, tests,
};

#[test]
fn merge_pull_request_review_state_page_counts_unresolved_threads_across_pages() {
	let first_page = tests::sample_pull_request_review_state_page(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		100,
		true,
		Some("cursor-1"),
	);
	let repository = tests::sample_pull_request_review_state_repository(first_page);
	let mut review_state = orchestrator::pull_request_review_state_from_page(
		&repository,
		repository.pull_request.as_ref().expect("pull request should exist"),
	)
	.expect("review state should build");
	let next_page = tests::sample_pull_request_review_state_page(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		1,
		false,
		None,
	);
	let next_repository = tests::sample_pull_request_review_state_repository(next_page);
	let next_cursor = orchestrator::merge_pull_request_review_state_page(
		&mut review_state,
		&next_repository,
		next_repository.pull_request.as_ref().expect("pull request should exist"),
	)
	.expect("page merge should succeed");

	assert_eq!(review_state.unresolved_review_threads, 101);
	assert_eq!(next_cursor, None);
}

#[test]
fn merge_pull_request_issue_comment_page_appends_comments_across_pages() {
	let first_page = tests::sample_pull_request_review_state_page(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		0,
		false,
		None,
	);
	let repository = tests::sample_pull_request_review_state_repository(first_page);
	let mut review_state = orchestrator::pull_request_review_state_from_page(
		&repository,
		repository.pull_request.as_ref().expect("pull request should exist"),
	)
	.expect("review state should build");
	let next_page = PullRequestIssueCommentsNode {
		url: String::from("https://github.com/hack-ink/decodex/pull/174"),
		comments: PullRequestIssueCommentConnection {
			nodes: vec![PullRequestIssueCommentNode {
				database_id: 501,
				body: String::from("Looks good"),
				created_at: String::from("2025-11-03T00:00:00Z"),
				author: Some(PullRequestActor {
					login: String::from(crate::orchestrator::EXTERNAL_REVIEW_ACTOR_LOGIN),
				}),
				reaction_groups: Vec::new(),
			}],
			page_info: PullRequestPageInfo { has_next_page: false, end_cursor: None },
		},
	};
	let next_cursor =
		orchestrator::merge_pull_request_issue_comment_page(&mut review_state, &next_page)
			.expect("comment page merge should succeed");

	assert_eq!(review_state.issue_comments.len(), 1);
	assert_eq!(review_state.issue_comments[0].database_id, 501);
	assert_eq!(next_cursor, None);
}

#[test]
fn merge_pull_request_review_state_page_rejects_changed_metadata_across_pages() {
	type ReviewPageMutation = fn(&mut PullRequestReviewStateNode);

	let cases: [(&str, ReviewPageMutation); 4] = [
		("review metadata", |page| {
			page.review_decision = Some(String::from("CHANGES_REQUESTED"));
		}),
		("pending review request count", |page| {
			page.review_requests.total_count = 1;
		}),
		("head repository owner", |page| {
			page.head_repository_owner =
				Some(PullRequestRepositoryOwner { login: String::from("someone-else") });
		}),
		("head repository name", |page| {
			page.head_repository =
				Some(PullRequestRepository { name: String::from("decodex-fork") });
		}),
	];

	for (case_name, mutate) in cases {
		super::assert_review_state_page_rejects_changed_metadata(case_name, mutate);
	}
}

#[test]
fn pull_request_review_state_query_requests_required_fields() {
	for expected_fragment in [
		"mergeCommitAllowed",
		"headRepository {\n        name\n      }",
		"comments(first: 100) {\n        nodes {\n          databaseId",
		"pageInfo {\n          hasNextPage\n          endCursor\n        }",
	] {
		assert!(
			orchestrator::PULL_REQUEST_REVIEW_STATE_QUERY.contains(expected_fragment),
			"query should include {expected_fragment}"
		);
	}
}

#[test]
fn next_pull_request_review_threads_cursor_requires_end_cursor_when_pagination_continues() {
	let page = tests::sample_pull_request_review_state_page(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		100,
		true,
		None,
	);
	let error = orchestrator::next_pull_request_review_threads_cursor(&page)
		.expect_err("missing end cursor should fail");

	assert!(error.to_string().contains("without an end cursor"));
}

#[test]
fn next_pull_request_issue_comments_cursor_requires_end_cursor_when_pagination_continues() {
	let comments = PullRequestIssueCommentConnection {
		nodes: Vec::new(),
		page_info: PullRequestPageInfo { has_next_page: true, end_cursor: None },
	};
	let error = orchestrator::next_pull_request_issue_comments_cursor(
		&comments,
		"https://github.com/hack-ink/decodex/pull/174",
	)
	.expect_err("missing end cursor should fail");

	assert!(error.to_string().contains("without an end cursor"));
}
