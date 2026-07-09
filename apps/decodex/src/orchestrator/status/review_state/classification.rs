use crate::{
	orchestrator::status::{
		self, OperatorPostReviewLaneStatus, PostReviewLaneClassification, PostReviewLaneDecision,
		PostReviewReadbackDegradation, PullRequestReadbackRootCause, PullRequestReviewState,
		ServiceConfig, TrackerIssue, WorktreeMapping,
	},
	state::ReviewLifecycleRecord,
};

pub(crate) fn initial_post_review_lane_classification(
	review_state: &PullRequestReviewState,
) -> PostReviewLaneClassification {
	PostReviewLaneClassification {
		decision: PostReviewLaneDecision::WaitForReview,
		reason: String::from("waiting_for_review_or_checks"),
		pr_url: Some(review_state.url.clone()),
		pr_head_sha: Some(review_state.head_ref_oid.clone()),
		pr_state: Some(review_state.state.clone()),
		review_decision: review_state.review_decision.clone(),
		mergeable: Some(review_state.mergeable.clone()),
		check_state: review_state.status_check_rollup_state.clone(),
		unresolved_review_threads: Some(review_state.unresolved_review_threads),
		readback_warning: None,
		readback_root_cause: None,
	}
}

pub(crate) fn blocked_post_review_lane_from_state(
	review_state: &PullRequestReviewState,
	reason: &str,
) -> PostReviewLaneClassification {
	let mut classification = initial_post_review_lane_classification(review_state);

	classification.decision = PostReviewLaneDecision::Block;
	classification.reason = reason.to_owned();
	classification.readback_root_cause = post_review_readback_root_cause_for_reason(reason)
		.map(|root_cause| root_cause.as_str().to_owned());

	classification
}

pub(crate) fn blocked_post_review_lane(reason: &str) -> PostReviewLaneClassification {
	PostReviewLaneClassification {
		decision: PostReviewLaneDecision::Block,
		reason: reason.to_owned(),
		pr_url: None,
		pr_head_sha: None,
		pr_state: None,
		review_decision: None,
		mergeable: None,
		check_state: None,
		unresolved_review_threads: None,
		readback_warning: None,
		readback_root_cause: post_review_readback_root_cause_for_reason(reason)
			.map(|root_cause| root_cause.as_str().to_owned()),
	}
}

pub(crate) fn blocked_post_review_lane_from_lifecycle(
	lifecycle_record: &ReviewLifecycleRecord,
	reason: &str,
) -> PostReviewLaneClassification {
	let mut classification = blocked_post_review_lane(reason);

	classification.pr_url = Some(lifecycle_record.pr_url().to_owned());
	classification.pr_head_sha = Some(lifecycle_record.pr_head_oid().to_owned());

	classification
}

pub(crate) fn readback_degraded_post_review_lane_from_lifecycle(
	lifecycle_record: &ReviewLifecycleRecord,
	root_cause: PullRequestReadbackRootCause,
) -> PostReviewLaneClassification {
	PostReviewReadbackDegradation::pull_request_state_from_lifecycle(lifecycle_record, root_cause)
		.wait_for_review_classification(None)
}

pub(crate) fn blocked_post_review_lane_status(
	project: &ServiceConfig,
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	reason: &str,
) -> OperatorPostReviewLaneStatus {
	OperatorPostReviewLaneStatus {
		project_id: project.service_id().to_owned(),
		issue_id: issue.id.clone(),
		issue_identifier: issue.identifier.clone(),
		issue_state: issue.state.name.clone(),
		branch_name: worktree.branch_name().to_owned(),
		worktree_path: status::relative_worktree_path_for_path(project, worktree.worktree_path()),
		classification: String::from("blocked"),
		reason: String::from(reason),
		pr_url: None,
		pr_head_sha: None,
		pr_state: None,
		review_decision: None,
		mergeable: None,
		check_state: None,
		unresolved_review_threads: None,
		shadowed_by_current_lane: false,
		readback_warning: None,
		readback_root_cause: post_review_readback_root_cause_for_reason(reason)
			.map(|root_cause| root_cause.as_str().to_owned()),
		loop_status: None,
	}
}

fn post_review_readback_root_cause_for_reason(
	reason: &str,
) -> Option<PullRequestReadbackRootCause> {
	match reason {
		"pull_request_repository_parse_failed" =>
			Some(PullRequestReadbackRootCause::PullRequestShapeReadFailed),
		"pull_request_branch_mismatch"
		| "pull_request_head_mismatch"
		| "pull_request_head_repository_name_mismatch"
		| "pull_request_head_repository_owner_mismatch"
		| "pull_request_merge_commit_lineage_check_failed"
		| "lifecycle_record_lineage_check_failed"
		| "lifecycle_record_lineage_mismatch"
		| "review_lifecycle_authority_branch_mismatch"
		| "review_lifecycle_authority_head_mismatch"
		| "review_lifecycle_authority_pr_mismatch" =>
			Some(PullRequestReadbackRootCause::LineageValidationFailed),
		_ => None,
	}
}
