use crate::{
	orchestrator::status::{
		ServiceConfig, StateStore, WorkflowDocument, post_review,
		post_review::{
			OperatorPostReviewLaneStatus, PostReviewLaneBuildContext, PostReviewLaneSnapshot,
			PullRequestReviewStateInspector, TrackerIssue, WorktreeMapping, lanes::status,
		},
	},
	prelude::Result,
};

pub(crate) fn build_post_review_lane_statuses_from_worktree_issues<I>(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
	worktree_issues: Vec<(WorktreeMapping, TrackerIssue)>,
) -> Result<Vec<OperatorPostReviewLaneStatus>>
where
	I: PullRequestReviewStateInspector,
{
	let tracker_policy = workflow.frontmatter().tracker();
	let success_state = tracker_policy.success_state();
	let completed_state = tracker_policy.resolved_completed_state();
	let lane_context = PostReviewLaneBuildContext {
		project,
		workflow,
		state_store,
		review_state_inspector,
		success_state,
		completed_state,
	};
	let mut lanes = Vec::new();

	for (worktree, issue) in worktree_issues {
		let Some(lane) = build_post_review_lane_status(&lane_context, issue, worktree)? else {
			continue;
		};

		lanes.push(lane);
	}

	lanes.sort_by(|left, right| left.issue_identifier.cmp(&right.issue_identifier));

	Ok(lanes)
}

fn build_post_review_lane_status<I>(
	context: &PostReviewLaneBuildContext<'_, I>,
	issue: TrackerIssue,
	worktree: WorktreeMapping,
) -> Result<Option<OperatorPostReviewLaneStatus>>
where
	I: PullRequestReviewStateInspector,
{
	if issue.state.name != context.success_state && issue.state.name != context.completed_state {
		return Ok(None);
	}

	if let Some(reason) = status::post_review_lane_static_block_reason(&issue, context.workflow)? {
		return Ok(Some(post_review::blocked_post_review_lane_status(
			context.project,
			&issue,
			&worktree,
			reason,
		)));
	}

	let retry_budget_exhausted = post_review::issue_retry_budget_exhausted_for_worktree(
		context.workflow,
		context.state_store,
		&issue.id,
		worktree.worktree_path(),
	)?;
	let lifecycle_record = context.state_store.review_lifecycle_record(
		context.project.service_id(),
		&issue.id,
		worktree.branch_name(),
	)?;

	if issue.state.name == context.completed_state && lifecycle_record.is_none() {
		return Ok(None);
	}
	let local_branch_name =
		match post_review::worktree_checkout_branch_name(worktree.worktree_path()) {
			Ok(local_branch_name) => local_branch_name,
			Err(_error) => {
				return Ok(Some(post_review::blocked_post_review_lane_status(
					context.project,
					&issue,
					&worktree,
					"worktree_checkout_branch_read_failed",
				)));
			},
		};
	let local_head_oid = match post_review::worktree_head_oid(worktree.worktree_path()) {
		Ok(local_head_oid) => local_head_oid,
		Err(_error) => {
			return Ok(Some(post_review::blocked_post_review_lane_status(
				context.project,
				&issue,
				&worktree,
				"worktree_head_read_failed",
			)));
		},
	};
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		lifecycle_record,
		local_branch_name,
		local_head_oid,
	};
	let mut classification = post_review::classify_post_review_lane_with_project(
		&snapshot,
		context.project,
		context.workflow,
		context.state_store,
		context.review_state_inspector,
	)?;

	if retry_budget_exhausted {
		classification = post_review::retry_budget_exhausted_post_review_lane_classification(
			&snapshot,
			context.project,
			context.workflow,
			context.review_state_inspector,
			classification,
		);
	}

	status::apply_active_ownership_warning_to_post_review_lane(
		context.project,
		context.success_state,
		&snapshot,
		&mut classification,
	);

	Ok(Some(status::post_review_lane_status_from_classification(
		context.project,
		context.state_store,
		&snapshot,
		classification,
	)?))
}
