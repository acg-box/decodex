use crate::orchestrator::status::{
	EXTERNAL_REVIEW_ACK_TIMEOUT_SECS, EXTERNAL_REVIEW_ACTOR_LOGIN, EXTERNAL_REVIEW_PASS_PHRASE,
	PullRequestReviewState,
};
use crate::state::ReviewLifecycleReadback;

pub(crate) fn request_comment_has_eyes(
	review_state: &PullRequestReviewState,
	marker: &impl ReviewLifecycleReadback,
) -> Option<bool> {
	let request_comment_id = marker.request_comment_database_id()?;

	Some(
		review_state
			.issue_comments
			.iter()
			.find(|comment| comment.database_id == request_comment_id)
			.is_some_and(|comment| comment.external_review_eyes_reaction_count > 0),
	)
}

pub(crate) fn request_ack_timed_out(
	marker: &impl ReviewLifecycleReadback,
	now_unix_epoch: i64,
) -> bool {
	let Some(request_created_at_unix_epoch) = marker.request_created_at_unix_epoch() else {
		return false;
	};

	now_unix_epoch - request_created_at_unix_epoch > EXTERNAL_REVIEW_ACK_TIMEOUT_SECS
		&& marker.request_retry_count() >= 1
}

pub(crate) fn external_review_result_arrived(
	review_state: &PullRequestReviewState,
	marker: &impl ReviewLifecycleReadback,
) -> bool {
	let Some(request_created_at_unix_epoch) = marker.request_created_at_unix_epoch() else {
		return false;
	};

	review_state.reviews.iter().any(|review| {
		review.submitted_at_unix_epoch >= request_created_at_unix_epoch
			&& is_external_review_actor_login(review.author_login.as_deref())
	}) || review_state.issue_comments.iter().any(|comment| {
		Some(comment.database_id) != marker.request_comment_database_id()
			&& comment.created_at_unix_epoch >= request_created_at_unix_epoch
			&& is_external_review_actor_login(comment.author_login.as_deref())
	})
}

pub(crate) fn external_review_has_strict_pass_signals(
	review_state: &PullRequestReviewState,
	marker: &impl ReviewLifecycleReadback,
) -> bool {
	let Some(request_created_at_unix_epoch) = marker.request_created_at_unix_epoch() else {
		return false;
	};
	let pass_phrase_seen_after_request = review_state.reviews.iter().any(|review| {
		review.submitted_at_unix_epoch >= request_created_at_unix_epoch
			&& is_external_review_actor_login(review.author_login.as_deref())
			&& external_review_body_is_strict_pass_signal(&review.body)
	}) || review_state.issue_comments.iter().any(|comment| {
		Some(comment.database_id) != marker.request_comment_database_id()
			&& comment.created_at_unix_epoch >= request_created_at_unix_epoch
			&& is_external_review_actor_login(comment.author_login.as_deref())
			&& external_review_body_is_strict_pass_signal(&comment.body)
	});

	pass_phrase_seen_after_request
		&& review_state.issue_description_external_review_thumbs_up_count > 0
}

pub(crate) fn external_review_has_actionable_feedback(
	review_state: &PullRequestReviewState,
	marker: &impl ReviewLifecycleReadback,
) -> bool {
	let Some(request_created_at_unix_epoch) = marker.request_created_at_unix_epoch() else {
		return false;
	};

	review_state.reviews.iter().any(|review| {
		review.submitted_at_unix_epoch >= request_created_at_unix_epoch
			&& is_external_review_actor_login(review.author_login.as_deref())
			&& matches!(review.state.as_str(), "COMMENTED" | "CHANGES_REQUESTED")
			&& external_review_body_has_actionable_feedback(&review.body)
	}) || review_state.issue_comments.iter().any(|comment| {
		Some(comment.database_id) != marker.request_comment_database_id()
			&& comment.created_at_unix_epoch >= request_created_at_unix_epoch
			&& is_external_review_actor_login(comment.author_login.as_deref())
			&& external_review_body_has_actionable_feedback(&comment.body)
	})
}

pub(crate) fn is_external_review_actor_login(login: Option<&str>) -> bool {
	login.is_some_and(|login| login.eq_ignore_ascii_case(EXTERNAL_REVIEW_ACTOR_LOGIN))
}

pub(crate) fn external_review_body_is_strict_pass_signal(body: &str) -> bool {
	body.trim() == EXTERNAL_REVIEW_PASS_PHRASE
}

pub(crate) fn external_review_body_has_actionable_feedback(body: &str) -> bool {
	let trimmed = body.trim();

	!trimmed.is_empty() && !external_review_body_is_strict_pass_signal(trimmed)
}
