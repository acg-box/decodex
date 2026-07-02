use std::path::Path;

use crate::{
	orchestrator::{
		PostReviewLaneClassification, PostReviewLaneDecision, PostReviewLaneSnapshot,
		PostReviewLaneStateLoad, PullRequestReviewStateInspector, RetainedReviewLaneBlocked,
		RetainedReviewLaneLoad, RetainedReviewRunIdentity,
		retained_review_orchestration::{
			RetainedReviewLane, markers, model::RetainedReviewLaneReviewLoad,
		},
		status,
	},
	prelude::Result,
	state::{self, ReviewHandoffMarker, StateStore, WorktreeMapping},
	tracker::{self, IssueTracker, TrackerIssue},
};

pub(super) fn eligible_post_review_orchestration_issue<T>(
	tracker: &T,
	issue: &TrackerIssue,
	service_id: &str,
	success_state: &str,
	opt_out_label: &str,
	needs_attention_label: &str,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	Ok(tracker::issue_has_label_with_server_confirmation(
		tracker,
		issue,
		&tracker::automation_active_label(service_id),
	)? && issue.state.name == success_state
		&& !issue.has_label(opt_out_label)
		&& !issue.has_label(needs_attention_label))
}

pub(super) fn load_retained_review_lane<I>(
	project_id: &str,
	state_store: &StateStore,
	issue: TrackerIssue,
	worktree: WorktreeMapping,
	review_state_inspector: &I,
) -> Result<RetainedReviewLaneLoad>
where
	I: PullRequestReviewStateInspector,
{
	let review_handoff =
		state_store.review_handoff_marker(project_id, &issue.id, worktree.branch_name())?;
	let Some(review_handoff) = review_handoff else {
		return Ok(blocked_retained_review_lane(
			issue,
			worktree,
			None,
			"missing_review_handoff_record",
		));
	};
	let local_branch_name = match status::worktree_checkout_branch_name(worktree.worktree_path()) {
		Ok(local_branch_name) => local_branch_name,
		Err(_error) => {
			return Ok(RetainedReviewLaneLoad::Wait(String::from(
				"worktree_checkout_branch_read_failed",
			)));
		},
	};
	let Some(local_branch_name) = local_branch_name else {
		return Ok(blocked_retained_review_lane(
			issue,
			worktree,
			Some(&review_handoff),
			"worktree_checkout_branch_missing",
		));
	};
	let local_head_oid = match status::worktree_head_oid(worktree.worktree_path()) {
		Ok(local_head_oid) => local_head_oid,
		Err(_error) => {
			return Ok(RetainedReviewLaneLoad::Wait(String::from("worktree_head_read_failed")));
		},
	};
	let Some(local_head_oid) = local_head_oid else {
		return Ok(blocked_retained_review_lane(
			issue,
			worktree,
			Some(&review_handoff),
			"worktree_head_missing",
		));
	};
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		review_handoff: Some(review_handoff.clone()),
		local_branch_name: Some(local_branch_name),
		local_head_oid: Some(local_head_oid.clone()),
	};
	let review_state =
		match load_retained_review_lane_review_state(&snapshot, review_state_inspector)? {
			RetainedReviewLaneReviewLoad::Skip => return Ok(RetainedReviewLaneLoad::Skip),
			RetainedReviewLaneReviewLoad::Blocked(reason) => {
				return Ok(blocked_retained_review_lane(
					snapshot.issue,
					snapshot.worktree,
					Some(&review_handoff),
					&reason,
				));
			},
			RetainedReviewLaneReviewLoad::ReviewState(review_state) => *review_state,
		};
	let orchestration_marker = markers::ensure_review_orchestration_marker(
		project_id,
		state_store,
		&snapshot.issue,
		&review_handoff,
		&local_head_oid,
	)?;

	Ok(RetainedReviewLaneLoad::Ready(Box::new(RetainedReviewLane {
		snapshot,
		review_state,
		orchestration_marker,
	})))
}

fn load_retained_review_lane_review_state<I>(
	snapshot: &PostReviewLaneSnapshot,
	review_state_inspector: &I,
) -> Result<RetainedReviewLaneReviewLoad>
where
	I: PullRequestReviewStateInspector,
{
	let review_state =
		match status::load_post_review_lane_review_state(snapshot, review_state_inspector)? {
			PostReviewLaneStateLoad::Classification(classification) => {
				return Ok(retained_review_lane_review_load_from_classification(classification));
			},
			PostReviewLaneStateLoad::ReviewState(review_state) => Box::new(review_state),
		};

	if review_state.state == "MERGED" {
		return Ok(RetainedReviewLaneReviewLoad::Skip);
	}
	if review_state.state != "OPEN" {
		return Ok(RetainedReviewLaneReviewLoad::Blocked(String::from("pull_request_not_open")));
	}
	if review_state.is_draft {
		return Ok(RetainedReviewLaneReviewLoad::Blocked(String::from("pull_request_is_draft")));
	}

	Ok(RetainedReviewLaneReviewLoad::ReviewState(review_state))
}

fn retained_review_lane_review_load_from_classification(
	classification: PostReviewLaneClassification,
) -> RetainedReviewLaneReviewLoad {
	if classification.decision == PostReviewLaneDecision::Block {
		RetainedReviewLaneReviewLoad::Blocked(classification.reason)
	} else {
		RetainedReviewLaneReviewLoad::Skip
	}
}

fn blocked_retained_review_lane(
	issue: TrackerIssue,
	worktree: WorktreeMapping,
	review_handoff: Option<&ReviewHandoffMarker>,
	reason: &str,
) -> RetainedReviewLaneLoad {
	let (run_id, attempt_number) =
		retained_review_run_identity(worktree.worktree_path(), review_handoff);

	RetainedReviewLaneLoad::Blocked(Box::new(RetainedReviewLaneBlocked {
		issue,
		worktree,
		run_identity: RetainedReviewRunIdentity { run_id, attempt_number },
		reason: reason.to_owned(),
	}))
}

fn retained_review_run_identity(
	worktree_path: &Path,
	review_handoff: Option<&ReviewHandoffMarker>,
) -> (String, i64) {
	if let Some(review_handoff) = review_handoff {
		return (review_handoff.run_id().to_owned(), review_handoff.attempt_number());
	}
	if let Ok(Some(marker)) = state::read_run_activity_marker_snapshot(worktree_path) {
		return (marker.run_id().to_owned(), marker.attempt_number());
	}

	(String::from("retained-review-orchestration"), 1)
}
