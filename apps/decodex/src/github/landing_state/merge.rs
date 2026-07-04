use crate::{
	github::landing_state::model::{
		PullRequestLandingStateNode, PullRequestReviewThreadConnection,
	},
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
};

pub(in crate::github::landing_state) fn pull_request_landing_state_from_page(
	pull_request: &PullRequestLandingStateNode,
) -> PullRequestLandingState {
	PullRequestLandingState {
		url: pull_request.url.clone(),
		state: pull_request.state.clone(),
		is_draft: pull_request.is_draft,
		review_decision: pull_request.review_decision.clone(),
		base_ref_name: pull_request.base_ref_name.clone(),
		pending_review_requests: pull_request.review_requests.total_count,
		mergeable: pull_request.mergeable.clone(),
		merge_state_status: pull_request.merge_state_status.clone(),
		head_ref_name: pull_request.head_ref_name.clone(),
		head_ref_oid: pull_request.head_ref_oid.clone(),
		status_check_rollup_state: pull_request
			.commits
			.nodes
			.first()
			.and_then(|node| node.commit.status_check_rollup.as_ref())
			.map(|rollup| rollup.state.clone()),
		unresolved_review_threads: count_unresolved_review_threads(&pull_request.review_threads),
	}
}

pub(in crate::github::landing_state) fn merge_pull_request_landing_state_page(
	landing_state: &mut PullRequestLandingState,
	pull_request: &PullRequestLandingStateNode,
) -> Result<Option<String>> {
	let page_state = pull_request_landing_state_from_page(pull_request);

	if landing_state.url != page_state.url
		|| landing_state.state != page_state.state
		|| landing_state.is_draft != page_state.is_draft
		|| landing_state.review_decision != page_state.review_decision
		|| landing_state.base_ref_name != page_state.base_ref_name
		|| landing_state.pending_review_requests != page_state.pending_review_requests
		|| landing_state.mergeable != page_state.mergeable
		|| landing_state.merge_state_status != page_state.merge_state_status
		|| landing_state.head_ref_name != page_state.head_ref_name
		|| landing_state.head_ref_oid != page_state.head_ref_oid
		|| landing_state.status_check_rollup_state != page_state.status_check_rollup_state
	{
		eyre::bail!("Pull request landing state changed while paginating `{}`.", landing_state.url);
	}

	landing_state.unresolved_review_threads += page_state.unresolved_review_threads;

	next_pull_request_review_threads_cursor(pull_request, landing_state.url.as_str())
}

pub(in crate::github::landing_state) fn next_pull_request_review_threads_cursor(
	pull_request: &PullRequestLandingStateNode,
	pr_url: &str,
) -> Result<Option<String>> {
	if !pull_request.review_threads.page_info.has_next_page {
		return Ok(None);
	}

	pull_request
		.review_threads
		.page_info
		.end_cursor
		.clone()
		.map(Some)
		.ok_or_else(|| {
			eyre::eyre!(
				"GitHub GraphQL response for `{pr_url}` reported additional review thread pages without an end cursor."
			)
		})
}

fn count_unresolved_review_threads(review_threads: &PullRequestReviewThreadConnection) -> usize {
	review_threads.nodes.iter().filter(|thread| !thread.is_resolved && !thread.is_outdated).count()
}
