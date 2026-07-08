use crate::orchestrator::{
	EXTERNAL_REVIEW_ACTOR_LOGIN, OffsetDateTime, PullRequestIssueCommentNode,
	PullRequestIssueCommentState, PullRequestReactionGroup, PullRequestReviewNode,
	PullRequestReviewState, PullRequestReviewStateNode, PullRequestReviewStateRepository,
	PullRequestReviewSummaryState, PullRequestReviewThreadConnection, Result, Rfc3339, eyre,
};

pub(crate) fn pull_request_review_state_from_page(
	repository: &PullRequestReviewStateRepository,
	pull_request: &PullRequestReviewStateNode,
) -> Result<PullRequestReviewState> {
	Ok(PullRequestReviewState {
		url: pull_request.url.clone(),
		state: pull_request.state.clone(),
		is_draft: pull_request.is_draft,
		review_decision: pull_request.review_decision.clone(),
		merge_commit_allowed: repository.merge_commit_allowed,
		pending_review_requests: pull_request.review_requests.total_count,
		mergeable: pull_request.mergeable.clone(),
		merge_state_status: pull_request.merge_state_status.clone(),
		base_ref_oid: pull_request.base_ref_oid.clone(),
		head_ref_name: pull_request.head_ref_name.clone(),
		head_ref_oid: pull_request.head_ref_oid.clone(),
		merge_commit_oid: pull_request.merge_commit.as_ref().map(|commit| commit.oid.clone()),
		head_repository_name: pull_request
			.head_repository
			.as_ref()
			.map(|repository| repository.name.clone()),
		head_repository_owner: pull_request
			.head_repository_owner
			.as_ref()
			.map(|owner| owner.login.clone()),
		status_check_rollup_state: pull_request_status_check_rollup_state(pull_request),
		required_status_contexts: Vec::new(),
		unresolved_review_threads: count_unresolved_review_threads(&pull_request.review_threads),
		issue_description_external_review_thumbs_up_count: reaction_group_actor_count(
			&pull_request.reaction_groups,
			"THUMBS_UP",
			EXTERNAL_REVIEW_ACTOR_LOGIN,
		),
		issue_comments: pull_request
			.comments
			.nodes
			.iter()
			.map(issue_comment_state_from_node)
			.collect::<Result<Vec<_>>>()?,
		reviews: pull_request
			.reviews
			.nodes
			.iter()
			.filter_map(review_summary_state_from_node)
			.collect(),
	})
}

pub(crate) fn count_unresolved_review_threads(
	review_threads: &PullRequestReviewThreadConnection,
) -> usize {
	review_threads.nodes.iter().filter(|thread| !thread.is_resolved && !thread.is_outdated).count()
}

pub(crate) fn pull_request_status_check_rollup_state(
	pull_request: &PullRequestReviewStateNode,
) -> Option<String> {
	pull_request
		.commits
		.nodes
		.first()
		.and_then(|node| node.commit.status_check_rollup.as_ref())
		.map(|rollup| rollup.state.clone())
}

pub(crate) fn issue_comment_state_from_node(
	comment: &PullRequestIssueCommentNode,
) -> Result<PullRequestIssueCommentState> {
	Ok(PullRequestIssueCommentState {
		database_id: comment.database_id,
		author_login: comment.author.as_ref().map(|author| author.login.clone()),
		body: comment.body.clone(),
		created_at_unix_epoch: parse_github_timestamp_to_unix_epoch(&comment.created_at)?,
		external_review_eyes_reaction_count: reaction_group_actor_count(
			&comment.reaction_groups,
			"EYES",
			EXTERNAL_REVIEW_ACTOR_LOGIN,
		),
	})
}

pub(crate) fn review_summary_state_from_node(
	review: &PullRequestReviewNode,
) -> Option<PullRequestReviewSummaryState> {
	let submitted_at_unix_epoch =
		parse_github_timestamp_to_unix_epoch(review.submitted_at.as_deref()?).ok()?;

	Some(PullRequestReviewSummaryState {
		author_login: review.author.as_ref().map(|author| author.login.clone()),
		body: review.body.clone(),
		state: review.state.clone(),
		submitted_at_unix_epoch,
	})
}

pub(crate) fn reaction_group_actor_count(
	groups: &[PullRequestReactionGroup],
	content: &str,
	actor_login: &str,
) -> usize {
	groups.iter().find(|group| group.content == content).map_or(0, |group| {
		group
			.users
			.nodes
			.iter()
			.filter(|actor| actor.login.eq_ignore_ascii_case(actor_login))
			.count()
	})
}

pub(crate) fn parse_github_timestamp_to_unix_epoch(timestamp: &str) -> Result<i64> {
	Ok(OffsetDateTime::parse(timestamp, &Rfc3339)
		.map_err(|error| eyre::eyre!("Failed to parse GitHub timestamp `{timestamp}`: {error}"))?
		.unix_timestamp())
}
