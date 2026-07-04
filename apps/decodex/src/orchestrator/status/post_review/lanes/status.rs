use crate::{
	orchestrator::status::{
		post_review,
		post_review::{
			OperatorLoopStatus, OperatorPostReviewLaneStatus, PostReviewLaneClassification,
			PostReviewLaneDecision, PostReviewLaneSnapshot, ServiceConfig, StateStore,
			TrackerIssue, WorkflowDocument,
		},
	},
	prelude::Result,
	tracker,
};

pub(in crate::orchestrator::status::post_review::lanes) fn apply_active_ownership_warning_to_post_review_lane(
	project: &ServiceConfig,
	success_state: &str,
	snapshot: &PostReviewLaneSnapshot,
	classification: &mut PostReviewLaneClassification,
) {
	if snapshot.review_handoff.is_none()
		|| snapshot.issue.state.name != success_state
		|| !snapshot.issue.labels_complete
		|| snapshot.issue.has_label(&tracker::automation_active_label(project.service_id()))
	{
		return;
	}
	if classification.readback_warning.is_none() {
		classification.readback_warning = Some(String::from("active_ownership_label_missing"));
	}
}

pub(in crate::orchestrator::status::post_review::lanes) fn post_review_lane_status_from_classification(
	project: &ServiceConfig,
	state_store: &StateStore,
	snapshot: &PostReviewLaneSnapshot,
	classification: PostReviewLaneClassification,
) -> Result<OperatorPostReviewLaneStatus> {
	let loop_status =
		operator_post_review_loop_status(project, state_store, snapshot, classification.decision)?;

	Ok(OperatorPostReviewLaneStatus {
		project_id: project.service_id().to_owned(),
		issue_id: snapshot.issue.id.clone(),
		issue_identifier: snapshot.issue.identifier.clone(),
		issue_state: snapshot.issue.state.name.clone(),
		branch_name: snapshot.worktree.branch_name().to_owned(),
		worktree_path: post_review::relative_worktree_path_for_path(
			project,
			snapshot.worktree.worktree_path(),
		),
		classification: classification.decision.as_str().to_owned(),
		reason: classification.reason,
		pr_url: classification.pr_url,
		pr_head_sha: classification.pr_head_sha,
		pr_state: classification.pr_state,
		review_decision: classification.review_decision,
		mergeable: classification.mergeable,
		check_state: classification.check_state,
		unresolved_review_threads: classification.unresolved_review_threads,
		shadowed_by_current_lane: false,
		readback_warning: classification.readback_warning,
		readback_root_cause: classification.readback_root_cause,
		loop_status,
	})
}

pub(in crate::orchestrator::status::post_review::lanes) fn post_review_lane_static_block_reason(
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
) -> Result<Option<&'static str>> {
	let tracker_policy = workflow.frontmatter().tracker();

	if issue.has_label(tracker_policy.opt_out_label()) {
		return Ok(Some("issue_opted_out"));
	}
	if issue.has_label(tracker_policy.needs_attention_label()) {
		return Ok(Some("issue_needs_attention"));
	}

	Ok(None)
}

fn operator_post_review_loop_status(
	project: &ServiceConfig,
	state_store: &StateStore,
	snapshot: &PostReviewLaneSnapshot,
	decision: PostReviewLaneDecision,
) -> Result<Option<OperatorLoopStatus>> {
	let Some(review_handoff) = snapshot.review_handoff.as_ref() else {
		return Ok(None);
	};
	let default_review_phase = match decision {
		PostReviewLaneDecision::ReadyToLand | PostReviewLaneDecision::WaitForReview => None,
		_ => Some("repair"),
	};

	post_review::operator_loop_status_for_run(
		project,
		state_store,
		&snapshot.issue.id,
		review_handoff.run_id(),
		review_handoff.attempt_number(),
		default_review_phase,
		None,
	)
	.map(Some)
}
