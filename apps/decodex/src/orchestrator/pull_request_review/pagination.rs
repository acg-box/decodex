use crate::orchestrator::{
	EXTERNAL_REVIEW_ACTOR_LOGIN, HashSet, PullRequestIssueCommentConnection,
	PullRequestIssueCommentsNode, PullRequestReviewState, PullRequestReviewStateNode,
	PullRequestReviewStateRepository, Result, eyre, pull_request_review,
};

pub(crate) fn merge_pull_request_review_state_page(
	review_state: &mut PullRequestReviewState,
	repository: &PullRequestReviewStateRepository,
	pull_request: &PullRequestReviewStateNode,
) -> Result<Option<String>> {
	if review_state.url != pull_request.url
		|| review_state.state != pull_request.state
		|| review_state.is_draft != pull_request.is_draft
		|| review_state.review_decision != pull_request.review_decision
		|| review_state.merge_commit_allowed != repository.merge_commit_allowed
		|| review_state.pending_review_requests != pull_request.review_requests.total_count
		|| review_state.mergeable != pull_request.mergeable
		|| review_state.merge_state_status != pull_request.merge_state_status
		|| review_state.base_ref_oid != pull_request.base_ref_oid
		|| review_state.head_ref_name != pull_request.head_ref_name
		|| review_state.head_ref_oid != pull_request.head_ref_oid
		|| review_state.merge_commit_oid
			!= pull_request.merge_commit.as_ref().map(|commit| commit.oid.clone())
		|| review_state.head_repository_name
			!= pull_request.head_repository.as_ref().map(|repository| repository.name.clone())
		|| review_state.head_repository_owner
			!= pull_request.head_repository_owner.as_ref().map(|owner| owner.login.clone())
		|| review_state.status_check_rollup_state
			!= pull_request_review::pull_request_status_check_rollup_state(pull_request)
		|| review_state.issue_description_external_review_thumbs_up_count
			!= pull_request_review::reaction_group_actor_count(
				&pull_request.reaction_groups,
				"THUMBS_UP",
				EXTERNAL_REVIEW_ACTOR_LOGIN,
			) || review_state.reviews
		!= pull_request
			.reviews
			.nodes
			.iter()
			.filter_map(pull_request_review::review_summary_state_from_node)
			.collect::<Vec<_>>()
	{
		eyre::bail!("Pull request review state changed while paginating `{}`.", review_state.url);
	}

	review_state.unresolved_review_threads +=
		pull_request_review::count_unresolved_review_threads(&pull_request.review_threads);

	next_pull_request_review_threads_cursor(pull_request)
}

pub(crate) fn merge_pull_request_issue_comment_page(
	review_state: &mut PullRequestReviewState,
	pull_request: &PullRequestIssueCommentsNode,
) -> Result<Option<String>> {
	if review_state.url != pull_request.url {
		eyre::bail!(
			"Pull request issue comment state changed while paginating `{}`.",
			review_state.url
		);
	}

	let mut comment_ids = review_state
		.issue_comments
		.iter()
		.map(|comment| comment.database_id)
		.collect::<HashSet<_>>();

	for comment in
		pull_request.comments.nodes.iter().map(pull_request_review::issue_comment_state_from_node)
	{
		let comment = comment?;

		if !comment_ids.insert(comment.database_id) {
			eyre::bail!(
				"Pull request issue comments repeated while paginating `{}`.",
				review_state.url
			);
		}

		review_state.issue_comments.push(comment);
	}

	next_pull_request_issue_comments_cursor(&pull_request.comments, pull_request.url.as_str())
}

pub(crate) fn next_pull_request_review_threads_cursor(
	pull_request: &PullRequestReviewStateNode,
) -> Result<Option<String>> {
	if !pull_request.review_threads.page_info.has_next_page {
		return Ok(None);
	}

	pull_request.review_threads.page_info.end_cursor.clone().map(Some).ok_or_else(|| {
		eyre::eyre!(
			"GitHub GraphQL response for `{}` reported additional review thread pages without an end cursor.",
			pull_request.url
		)
	})
}

pub(crate) fn next_pull_request_issue_comments_cursor(
	comments: &PullRequestIssueCommentConnection,
	pr_url: &str,
) -> Result<Option<String>> {
	if !comments.page_info.has_next_page {
		return Ok(None);
	}

	comments.page_info.end_cursor.clone().map(Some).ok_or_else(|| {
		eyre::eyre!(
			"GitHub GraphQL response for `{pr_url}` reported additional issue comment pages without an end cursor.",
		)
	})
}
