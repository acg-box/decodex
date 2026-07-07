use std::path::Path;

use crate::orchestrator::{
	github,
	status::{
		self, PostReviewLaneSnapshot, PostReviewLaneStateLoad, PullRequestReviewState,
		PullRequestReviewStateInspector,
	},
};

pub(crate) fn load_post_review_lane_review_state<I>(
	snapshot: &PostReviewLaneSnapshot,
	review_state_inspector: &I,
) -> crate::prelude::Result<PostReviewLaneStateLoad>
where
	I: PullRequestReviewStateInspector,
{
	if let Some(lifecycle_record) = snapshot.lifecycle_record.as_ref() {
		let local_head_oid =
			match status::validate_post_review_lane_worktree(snapshot, lifecycle_record) {
				Ok(local_head_oid) => local_head_oid,
				Err(reason) => {
					return Ok(PostReviewLaneStateLoad::Classification(
						status::blocked_post_review_lane_from_lifecycle(lifecycle_record, reason),
					));
				},
			};
		let review_state = match review_state_inspector.inspect_review_state_readback(
			snapshot.worktree.worktree_path(),
			lifecycle_record.pr_url(),
		) {
			Ok(review_state) => review_state,
			Err(error) => {
				return Ok(PostReviewLaneStateLoad::Classification(
					status::readback_degraded_post_review_lane_from_lifecycle(
						lifecycle_record,
						error.root_cause(),
					),
				));
			},
		};

		return Ok(validate_post_review_lane_review_state(
			review_state,
			snapshot.worktree.branch_name(),
			local_head_oid,
			snapshot.worktree.worktree_path(),
		));
	}

	Ok(PostReviewLaneStateLoad::Classification(status::blocked_post_review_lane(
		"missing_review_lifecycle_record",
	)))
}

pub(crate) fn validate_post_review_lane_review_state(
	review_state: PullRequestReviewState,
	expected_branch_name: &str,
	local_head_oid: &str,
	worktree_path: &Path,
) -> PostReviewLaneStateLoad {
	let Some(pr_owner) =
		github::parse_pull_request_url(&review_state.url).ok().map(|locator| locator.owner)
	else {
		return PostReviewLaneStateLoad::Classification(
			status::blocked_post_review_lane_from_state(
				&review_state,
				"pull_request_repository_parse_failed",
			),
		);
	};
	let Some(pr_repo) =
		github::parse_pull_request_url(&review_state.url).ok().map(|locator| locator.repo)
	else {
		return PostReviewLaneStateLoad::Classification(
			status::blocked_post_review_lane_from_state(
				&review_state,
				"pull_request_repository_parse_failed",
			),
		);
	};

	if review_state.head_repository_owner.as_deref() != Some(pr_owner.as_str()) {
		return PostReviewLaneStateLoad::Classification(
			status::blocked_post_review_lane_from_state(
				&review_state,
				"pull_request_head_repository_owner_mismatch",
			),
		);
	}
	if review_state.head_repository_name.as_deref() != Some(pr_repo.as_str()) {
		return PostReviewLaneStateLoad::Classification(
			status::blocked_post_review_lane_from_state(
				&review_state,
				"pull_request_head_repository_name_mismatch",
			),
		);
	}
	if review_state.head_ref_name != expected_branch_name {
		return PostReviewLaneStateLoad::Classification(
			status::blocked_post_review_lane_from_state(
				&review_state,
				"pull_request_branch_mismatch",
			),
		);
	}
	if review_state.head_ref_oid != local_head_oid {
		match merged_pr_local_head_matches_landed_lineage(
			worktree_path,
			&review_state,
			local_head_oid,
		) {
			Ok(true) => return PostReviewLaneStateLoad::ReviewState(review_state),
			Ok(false) => {},
			Err(reason) => {
				return PostReviewLaneStateLoad::Classification(
					status::blocked_post_review_lane_from_state(&review_state, reason),
				);
			},
		}

		return PostReviewLaneStateLoad::Classification(
			status::blocked_post_review_lane_from_state(
				&review_state,
				"pull_request_head_mismatch",
			),
		);
	}

	PostReviewLaneStateLoad::ReviewState(review_state)
}

pub(crate) fn merged_pr_local_head_matches_landed_lineage(
	worktree_path: &Path,
	review_state: &PullRequestReviewState,
	local_head_oid: &str,
) -> std::result::Result<bool, &'static str> {
	if review_state.state != "MERGED" {
		return Ok(false);
	}

	let Some(merge_commit_oid) = review_state.merge_commit_oid.as_deref() else {
		return Ok(false);
	};

	if merge_commit_oid == local_head_oid {
		return Ok(true);
	}

	status::worktree_head_descends_from_lifecycle_record(
		worktree_path,
		merge_commit_oid,
		local_head_oid,
	)
	.map_err(|()| "pull_request_merge_commit_lineage_check_failed")
}
