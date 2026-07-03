use crate::orchestrator::tests::{
	EXTERNAL_REVIEW_ACTOR_LOGIN, EXTERNAL_REVIEW_PASS_PHRASE, EXTERNAL_REVIEW_REQUEST_BODY, Path,
	PullRequestCommitConnection, PullRequestCommitNode, PullRequestCommitPayload,
	PullRequestIssueCommentConnection, PullRequestIssueCommentState, PullRequestPageInfo,
	PullRequestRepository, PullRequestRepositoryOwner, PullRequestReviewConnection,
	PullRequestReviewRequestConnection, PullRequestReviewState, PullRequestReviewStateInspector,
	PullRequestReviewStateNode, PullRequestReviewStateRepository, PullRequestReviewSummaryState,
	PullRequestReviewThreadConnection, PullRequestReviewThreadNode, PullRequestStatusCheckRollup,
	RefCell, Result, TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID,
	TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT, github,
};

pub(super) struct FakePullRequestReviewStateInspector {
	responses: RefCell<Vec<Result<PullRequestReviewState>>>,
}
impl FakePullRequestReviewStateInspector {
	pub(super) fn new(responses: Vec<Result<PullRequestReviewState>>) -> Self {
		Self { responses: RefCell::new(responses) }
	}
}

impl PullRequestReviewStateInspector for FakePullRequestReviewStateInspector {
	fn inspect_review_state(&self, _cwd: &Path, _pr_url: &str) -> Result<PullRequestReviewState> {
		self.responses.borrow_mut().remove(0)
	}
}

pub(super) fn add_external_review_ack(review_state: &mut PullRequestReviewState) {
	add_review_request_ack_from_actor(review_state, EXTERNAL_REVIEW_ACTOR_LOGIN);
}

pub(super) fn add_review_request_ack_from_actor(
	review_state: &mut PullRequestReviewState,
	actor_login: &str,
) {
	review_state.issue_comments.push(PullRequestIssueCommentState {
		database_id: TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID,
		author_login: Some(actor_login.to_owned()),
		body: String::from(EXTERNAL_REVIEW_REQUEST_BODY),
		created_at_unix_epoch: TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT,
		external_review_eyes_reaction_count: usize::from(
			actor_login.eq_ignore_ascii_case(EXTERNAL_REVIEW_ACTOR_LOGIN),
		),
	});
}

pub(super) fn add_external_review_summary(
	review_state: &mut PullRequestReviewState,
	body: &str,
	state: &str,
	submitted_at_unix_epoch: i64,
) {
	add_review_summary_from_actor(
		review_state,
		EXTERNAL_REVIEW_ACTOR_LOGIN,
		body,
		state,
		submitted_at_unix_epoch,
	);
}

pub(super) fn add_review_summary_from_actor(
	review_state: &mut PullRequestReviewState,
	actor_login: &str,
	body: &str,
	state: &str,
	submitted_at_unix_epoch: i64,
) {
	review_state.reviews.push(PullRequestReviewSummaryState {
		author_login: Some(actor_login.to_owned()),
		body: body.to_owned(),
		state: state.to_owned(),
		submitted_at_unix_epoch,
	});
}

pub(super) fn add_external_review_pass(review_state: &mut PullRequestReviewState) {
	add_external_review_pass_from_actor(review_state, EXTERNAL_REVIEW_ACTOR_LOGIN);
}

pub(super) fn add_external_review_pass_from_actor(
	review_state: &mut PullRequestReviewState,
	actor_login: &str,
) {
	if actor_login.eq_ignore_ascii_case(EXTERNAL_REVIEW_ACTOR_LOGIN) {
		review_state.issue_description_external_review_thumbs_up_count += 1;
	}

	add_review_summary_from_actor(
		review_state,
		actor_login,
		EXTERNAL_REVIEW_PASS_PHRASE,
		"APPROVED",
		TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT + 1,
	);
}

pub(super) fn add_external_review_findings(review_state: &mut PullRequestReviewState, body: &str) {
	add_review_summary_from_actor(
		review_state,
		EXTERNAL_REVIEW_ACTOR_LOGIN,
		body,
		"COMMENTED",
		TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT + 1,
	);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sample_pull_request_review_state(
	pr_url: &str,
	branch_name: &str,
	head_oid: &str,
	review_decision: Option<&str>,
	mergeable: &str,
	merge_state_status: &str,
	check_state: Option<&str>,
	unresolved_review_threads: usize,
) -> PullRequestReviewState {
	sample_pull_request_review_state_with_pending_requests(
		pr_url,
		branch_name,
		head_oid,
		review_decision,
		mergeable,
		merge_state_status,
		check_state,
		unresolved_review_threads,
		0,
	)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sample_pull_request_review_state_with_pending_requests(
	pr_url: &str,
	branch_name: &str,
	head_oid: &str,
	review_decision: Option<&str>,
	mergeable: &str,
	merge_state_status: &str,
	check_state: Option<&str>,
	unresolved_review_threads: usize,
	pending_review_requests: usize,
) -> PullRequestReviewState {
	let head_repository_owner =
		github::parse_pull_request_url(pr_url).expect("pull request URL should parse").owner;

	PullRequestReviewState {
		url: pr_url.to_owned(),
		state: String::from("OPEN"),
		is_draft: false,
		review_decision: review_decision.map(str::to_owned),
		merge_commit_allowed: true,
		pending_review_requests,
		mergeable: mergeable.to_owned(),
		merge_state_status: merge_state_status.to_owned(),
		head_ref_name: branch_name.to_owned(),
		head_ref_oid: head_oid.to_owned(),
		merge_commit_oid: None,
		head_repository_name: Some(
			github::parse_pull_request_url(pr_url).expect("pull request URL should parse").repo,
		),
		head_repository_owner: Some(head_repository_owner),
		status_check_rollup_state: check_state.map(str::to_owned),
		unresolved_review_threads,
		issue_description_external_review_thumbs_up_count: 0,
		issue_comments: Vec::new(),
		reviews: Vec::new(),
	}
}

pub(super) fn sample_pull_request_review_state_page(
	pr_url: &str,
	branch_name: &str,
	head_oid: &str,
	unresolved_review_threads: usize,
	has_next_page: bool,
	end_cursor: Option<&str>,
) -> PullRequestReviewStateNode {
	let locator = github::parse_pull_request_url(pr_url).expect("pull request URL should parse");

	PullRequestReviewStateNode {
		url: pr_url.to_owned(),
		state: String::from("OPEN"),
		is_draft: false,
		review_decision: Some(String::from("APPROVED")),
		review_requests: PullRequestReviewRequestConnection { total_count: 0 },
		mergeable: String::from("MERGEABLE"),
		merge_state_status: String::from("CLEAN"),
		head_ref_name: branch_name.to_owned(),
		head_ref_oid: head_oid.to_owned(),
		merge_commit: None,
		head_repository: Some(PullRequestRepository { name: locator.repo }),
		head_repository_owner: Some(PullRequestRepositoryOwner { login: locator.owner }),
		reaction_groups: Vec::new(),
		comments: PullRequestIssueCommentConnection {
			nodes: Vec::new(),
			page_info: PullRequestPageInfo { has_next_page: false, end_cursor: None },
		},
		reviews: PullRequestReviewConnection { nodes: Vec::new() },
		review_threads: PullRequestReviewThreadConnection {
			nodes: (0..unresolved_review_threads)
				.map(|_| PullRequestReviewThreadNode { is_resolved: false, is_outdated: false })
				.collect(),
			page_info: PullRequestPageInfo {
				has_next_page,
				end_cursor: end_cursor.map(str::to_owned),
			},
		},
		commits: PullRequestCommitConnection {
			nodes: vec![PullRequestCommitNode {
				commit: PullRequestCommitPayload {
					status_check_rollup: Some(PullRequestStatusCheckRollup {
						state: String::from("SUCCESS"),
					}),
				},
			}],
		},
	}
}

pub(super) fn sample_pull_request_review_state_repository(
	pull_request: PullRequestReviewStateNode,
) -> PullRequestReviewStateRepository {
	PullRequestReviewStateRepository {
		merge_commit_allowed: true,
		pull_request: Some(pull_request),
	}
}
