use crate::orchestrator::{
	self, EXTERNAL_REVIEW_ACTOR_LOGIN, EXTERNAL_REVIEW_PASS_PHRASE, EXTERNAL_REVIEW_REQUEST_BODY,
	PullRequestActor, PullRequestIssueCommentNode, PullRequestReactionGroup,
	PullRequestReactionUsersConnection, PullRequestReviewNode,
	tests::{self, TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID, TEST_NON_EXTERNAL_REVIEW_ACTOR_LOGIN},
};

#[test]
fn pull_request_review_state_from_page_scopes_signals_to_external_review_actor() {
	let mut page = tests::sample_pull_request_review_state_page(
		"https://github.com/hack-ink/decodex/pull/173",
		"main",
		"deadbeef",
		0,
		false,
		None,
	);

	page.reaction_groups.push(PullRequestReactionGroup {
		content: String::from("THUMBS_UP"),
		users: PullRequestReactionUsersConnection {
			nodes: vec![
				PullRequestActor {
					login: String::from(crate::orchestrator::EXTERNAL_REVIEW_ACTOR_LOGIN),
				},
				PullRequestActor { login: String::from(TEST_NON_EXTERNAL_REVIEW_ACTOR_LOGIN) },
			],
		},
	});
	page.comments.nodes.push(PullRequestIssueCommentNode {
		database_id: TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID,
		body: String::from(EXTERNAL_REVIEW_REQUEST_BODY),
		created_at: String::from("2025-11-03T00:00:00Z"),
		author: Some(PullRequestActor { login: String::from("lane-owner") }),
		reaction_groups: vec![PullRequestReactionGroup {
			content: String::from("EYES"),
			users: PullRequestReactionUsersConnection {
				nodes: vec![
					PullRequestActor { login: String::from(TEST_NON_EXTERNAL_REVIEW_ACTOR_LOGIN) },
					PullRequestActor {
						login: String::from(crate::orchestrator::EXTERNAL_REVIEW_ACTOR_LOGIN),
					},
				],
			},
		}],
	});
	page.reviews.nodes.push(PullRequestReviewNode {
		body: String::from(EXTERNAL_REVIEW_PASS_PHRASE),
		state: String::from("APPROVED"),
		submitted_at: Some(String::from("2025-11-03T00:00:01Z")),
		author: Some(PullRequestActor {
			login: String::from(TEST_NON_EXTERNAL_REVIEW_ACTOR_LOGIN),
		}),
	});

	let repository = tests::sample_pull_request_review_state_repository(page);
	let review_state = orchestrator::pull_request_review_state_from_page(
		&repository,
		repository.pull_request.as_ref().expect("pull request should exist"),
	)
	.expect("review state should build");

	assert_eq!(review_state.issue_description_external_review_thumbs_up_count, 1);
	assert_eq!(review_state.issue_comments.len(), 1);
	assert_eq!(review_state.issue_comments[0].external_review_eyes_reaction_count, 1);
	assert_eq!(
		review_state.reviews[0].author_login.as_deref(),
		Some(TEST_NON_EXTERNAL_REVIEW_ACTOR_LOGIN)
	);
}

#[test]
fn pull_request_review_state_from_page_skips_pending_reviews_without_submitted_timestamp() {
	let mut page = tests::sample_pull_request_review_state_page(
		"https://github.com/hack-ink/decodex/pull/173",
		"main",
		"deadbeef",
		0,
		false,
		None,
	);

	page.reviews.nodes.push(PullRequestReviewNode {
		body: String::from("pending"),
		state: String::from("PENDING"),
		submitted_at: None,
		author: Some(PullRequestActor { login: String::from(EXTERNAL_REVIEW_ACTOR_LOGIN) }),
	});

	let repository = tests::sample_pull_request_review_state_repository(page);
	let review_state = orchestrator::pull_request_review_state_from_page(
		&repository,
		repository.pull_request.as_ref().expect("pull request should exist"),
	)
	.expect("review state should build");

	assert!(review_state.reviews.is_empty());
}
