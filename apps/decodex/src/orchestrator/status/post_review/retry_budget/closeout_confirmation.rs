use crate::state::ReviewLifecycleRecord;

use crate::orchestrator::status::post_review::{
	self, PostReviewLaneClassification, PostReviewLaneDecision, PostReviewLaneSnapshot,
	PullRequestMergeViewResponse, PullRequestReadbackRootCause, ServiceConfig, github,
	retry_budget,
};

pub(crate) fn confirm_status_visible_merged_closeout(
	snapshot: &PostReviewLaneSnapshot,
	project: &ServiceConfig,
	classification: &mut PostReviewLaneClassification,
) {
	if !retry_budget::merged_closeout_pending_classification(classification) {
		return;
	}

	let Some(pr_url) = classification.pr_url.as_deref() else {
		mark_merged_closeout_confirmation_conflict(classification, None, None);

		return;
	};
	let expected_head_sha = snapshot
		.lifecycle_record
		.as_ref()
		.map(ReviewLifecycleRecord::pr_head_oid)
		.or(classification.pr_head_sha.as_deref());
	let Some(expected_head_sha) = expected_head_sha else {
		mark_merged_closeout_confirmation_conflict(classification, None, None);

		return;
	};
	let github_token = match post_review::resolve_configured_env_var(
		"github.token_env_var",
		Some(project.github().token_env_var()),
	) {
		Ok(github_token) => github_token,
		Err(error) => {
			let root_cause = post_review::classify_pull_request_readback_report(&error);

			mark_merged_closeout_confirmation_conflict(classification, None, Some(root_cause));

			return;
		},
	};
	let merge_readback = match github::inspect_pull_request_merge_readback(
		snapshot.worktree.worktree_path(),
		pr_url,
		&github_token,
		project.github().command_path(),
	) {
		Ok(merge_readback) => merge_readback,
		Err(error) => {
			let root_cause = post_review::classify_pull_request_readback_report(&error);

			mark_merged_closeout_confirmation_conflict(classification, None, Some(root_cause));

			return;
		},
	};

	if merge_readback.state == "MERGED"
		&& merge_readback.head_ref_oid.as_deref() == Some(expected_head_sha)
	{
		return;
	}

	mark_merged_closeout_confirmation_conflict(
		classification,
		Some(merge_readback),
		Some(PullRequestReadbackRootCause::LineageValidationFailed),
	);
}

pub(crate) fn mark_merged_closeout_confirmation_conflict(
	classification: &mut PostReviewLaneClassification,
	merge_readback: Option<PullRequestMergeViewResponse>,
	root_cause: Option<PullRequestReadbackRootCause>,
) {
	classification.decision = PostReviewLaneDecision::Block;
	classification.reason = String::from("pull_request_merge_state_conflict");
	classification.readback_warning = Some(String::from("pull_request_merge_state_conflict"));
	classification.readback_root_cause =
		root_cause.map(|root_cause| root_cause.as_str().to_owned());

	if let Some(merge_readback) = merge_readback {
		classification.pr_state = Some(merge_readback.state);
		classification.pr_head_sha =
			merge_readback.head_ref_oid.or_else(|| classification.pr_head_sha.clone());
	}
}
