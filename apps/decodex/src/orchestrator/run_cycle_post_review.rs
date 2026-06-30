use std::{collections::HashMap, path::Path};

use super::{
	GhPullRequestReviewStateInspector, IssueDispatchMode, IssueTracker,
	OperatorPostReviewLaneStatus, PullRequestReviewStateInspector, RetainedReviewRunIdentity,
	SelectedIssueRunCandidate, ServiceConfig, StateStore, TERMINAL_GUARDED_RUN_STATUS,
	TrackerIssue, WorkflowDocument, WorktreeMapping, build_post_review_lane_statuses,
	closeout_lane_active_claim_blocks_dispatch,
};
use crate::{
	prelude::{Result, eyre},
	state,
};

pub(crate) fn select_post_review_issue_candidate<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	excluded_issue_ids: &[&str],
) -> Result<Option<SelectedIssueRunCandidate>>
where
	T: IssueTracker,
{
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
		github_command_path: project.github().command_path().map(Path::to_path_buf),
	};

	select_post_review_issue_candidate_with_inspector(
		tracker,
		project,
		workflow,
		state_store,
		excluded_issue_ids,
		&review_state_inspector,
	)
}

pub(crate) fn select_post_review_issue_candidate_with_inspector<T, I>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	excluded_issue_ids: &[&str],
	review_state_inspector: &I,
) -> Result<Option<SelectedIssueRunCandidate>>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
{
	if let Some(issue) = select_post_review_repair_issue_candidate_with_inspector(
		tracker,
		project,
		workflow,
		state_store,
		excluded_issue_ids,
		review_state_inspector,
	)? {
		return Ok(Some(SelectedIssueRunCandidate::new(issue, IssueDispatchMode::ReviewRepair)));
	}

	select_post_review_closeout_issue_candidate_with_inspector(
		tracker,
		project,
		workflow,
		state_store,
		excluded_issue_ids,
		review_state_inspector,
	)
}

pub(crate) fn select_post_review_repair_issue_candidate_with_inspector<T, I>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	excluded_issue_ids: &[&str],
	review_state_inspector: &I,
) -> Result<Option<TrackerIssue>>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
{
	let lanes = build_post_review_lane_statuses(
		tracker,
		project,
		workflow,
		state_store,
		review_state_inspector,
	)?;
	let candidate_issue_ids = lanes
		.iter()
		.filter(|lane| lane.classification == "needs_review_repair")
		.filter(|lane| !excluded_issue_ids.contains(&lane.issue_id.as_str()))
		.map(|lane| lane.issue_id.clone())
		.collect::<Vec<_>>();

	if candidate_issue_ids.is_empty() {
		return Ok(None);
	}

	let issues = tracker.refresh_issues(&candidate_issue_ids)?;
	let mut issues_by_id =
		issues.into_iter().map(|issue| (issue.id.clone(), issue)).collect::<HashMap<_, _>>();

	for lane in lanes {
		if lane.classification != "needs_review_repair" {
			continue;
		}
		if excluded_issue_ids.contains(&lane.issue_id.as_str()) {
			continue;
		}
		if state_store.issue_has_active_shared_claim(project.service_id(), &lane.issue_id)? {
			continue;
		}

		if let Some(issue) = issues_by_id.remove(&lane.issue_id) {
			return Ok(Some(issue));
		}
	}

	Ok(None)
}

pub(crate) fn select_post_review_closeout_issue_candidate_with_inspector<T, I>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	excluded_issue_ids: &[&str],
	review_state_inspector: &I,
) -> Result<Option<SelectedIssueRunCandidate>>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
{
	let completed_state = workflow.frontmatter().tracker().resolved_completed_state();
	let lanes = build_post_review_lane_statuses(
		tracker,
		project,
		workflow,
		state_store,
		review_state_inspector,
	)?;
	let candidate_issue_ids = lanes
		.iter()
		.filter(|lane| post_review_lane_is_closeout_candidate(lane, completed_state))
		.filter(|lane| !excluded_issue_ids.contains(&lane.issue_id.as_str()))
		.map(|lane| lane.issue_id.clone())
		.collect::<Vec<_>>();

	if candidate_issue_ids.is_empty() {
		return Ok(None);
	}

	let issues = tracker.refresh_issues(&candidate_issue_ids)?;
	let mut issues_by_id =
		issues.into_iter().map(|issue| (issue.id.clone(), issue)).collect::<HashMap<_, _>>();

	for lane in lanes {
		let is_closeout_candidate = post_review_lane_is_closeout_candidate(&lane, completed_state);

		if !is_closeout_candidate {
			continue;
		}
		if excluded_issue_ids.contains(&lane.issue_id.as_str()) {
			continue;
		}

		if let Some(issue) = issues_by_id.remove(&lane.issue_id) {
			if closeout_lane_active_claim_blocks_dispatch(project, state_store, &issue)? {
				continue;
			}

			let preferred_run_identity = retained_closeout_preferred_run_identity(
				state_store,
				project.service_id(),
				&issue,
			)?;

			return Ok(Some(SelectedIssueRunCandidate {
				issue,
				dispatch_mode: IssueDispatchMode::Closeout,
				preferred_run_identity,
			}));
		}
	}

	Ok(None)
}

pub(crate) fn retained_closeout_preferred_run_identity(
	state_store: &StateStore,
	project_id: &str,
	issue: &TrackerIssue,
) -> Result<Option<RetainedReviewRunIdentity>> {
	let Some(worktree) = state_store.worktree_for_issue(&issue.id)? else {
		return Ok(None);
	};
	let Some(review_handoff) =
		state_store.review_handoff_marker(project_id, &issue.id, worktree.branch_name())?
	else {
		return Ok(None);
	};
	let identity = RetainedReviewRunIdentity {
		run_id: review_handoff.run_id().to_owned(),
		attempt_number: review_handoff.attempt_number(),
	};

	if retained_closeout_run_identity_is_reusable(state_store, &issue.id, &identity)?
		|| retained_closeout_handoff_identity_is_reusable_after_parent_reconciliation(
			state_store,
			&issue.id,
			&identity,
			&worktree,
		)? {
		return Ok(Some(identity));
	}

	Ok(None)
}

pub(crate) fn retained_closeout_run_identity_is_reusable(
	state_store: &StateStore,
	issue_id: &str,
	identity: &RetainedReviewRunIdentity,
) -> Result<bool> {
	if state_store.issue_has_retry_budget_attempt_after(issue_id, identity.attempt_number)? {
		return Ok(false);
	}

	let Some(existing_attempt) = state_store.run_attempt(&identity.run_id)? else {
		return Ok(true);
	};

	if existing_attempt.issue_id() != issue_id
		|| existing_attempt.attempt_number() != identity.attempt_number
	{
		return Ok(false);
	}

	Ok(!matches!(existing_attempt.status(), "failed" | "interrupted" | TERMINAL_GUARDED_RUN_STATUS))
}

fn retained_closeout_handoff_identity_is_reusable_after_parent_reconciliation(
	state_store: &StateStore,
	issue_id: &str,
	identity: &RetainedReviewRunIdentity,
	worktree: &WorktreeMapping,
) -> Result<bool> {
	if state_store.issue_has_retry_budget_attempt_after(issue_id, identity.attempt_number)? {
		return Ok(false);
	}

	let Some(existing_attempt) = state_store.run_attempt(&identity.run_id)? else {
		return Ok(false);
	};

	if existing_attempt.issue_id() != issue_id
		|| existing_attempt.attempt_number() != identity.attempt_number
	{
		return Ok(false);
	}
	if !matches!(existing_attempt.status(), "failed" | "interrupted") {
		return Ok(false);
	}
	if worktree_has_retry_schedule_for_run(worktree.worktree_path(), identity)? {
		return Ok(false);
	}

	Ok(true)
}

fn worktree_has_retry_schedule_for_run(
	worktree_path: &Path,
	identity: &RetainedReviewRunIdentity,
) -> Result<bool> {
	let Some(marker) = state::read_run_activity_marker_snapshot(worktree_path)? else {
		return Ok(false);
	};

	Ok(marker.run_id() == identity.run_id
		&& marker.attempt_number() == identity.attempt_number
		&& marker.retry_kind().is_some())
}

pub(crate) fn post_review_lane_is_closeout_candidate(
	lane: &OperatorPostReviewLaneStatus,
	_completed_state: &str,
) -> bool {
	lane.classification == "continue" && lane.reason == "pull_request_merged_closeout_pending"
}

pub(crate) fn post_review_lane_is_repair_candidate(lane: &OperatorPostReviewLaneStatus) -> bool {
	lane.classification == "needs_review_repair"
}

pub(crate) fn select_target_post_review_repair_issue_candidate_with_inspector<T, I>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	target_issue_id: &str,
	target_issue_reference: &str,
	review_state_inspector: &I,
) -> Result<Option<TrackerIssue>>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
{
	let lanes = build_post_review_lane_statuses(
		tracker,
		project,
		workflow,
		state_store,
		review_state_inspector,
	)?;
	let repair_lanes =
		lanes.into_iter().filter(post_review_lane_is_repair_candidate).collect::<Vec<_>>();

	if repair_lanes.is_empty() {
		return Ok(None);
	}

	let Some(target_lane) = repair_lanes.iter().find(|lane| lane.issue_id == target_issue_id)
	else {
		let visible_lanes = repair_lanes
			.iter()
			.map(|lane| lane.issue_identifier.as_str())
			.collect::<Vec<_>>()
			.join(", ");

		eyre::bail!(
			"targeted retained review repair mismatch: requested issue `{}` does not match status-visible retained review repair lane(s) `{}`",
			target_issue_reference,
			visible_lanes,
		);
	};
	let issue_ids = [target_lane.issue_id.clone()];
	let mut issues = tracker.refresh_issues(&issue_ids)?;
	let Some(issue_index) = issues.iter().position(|issue| issue.id == target_lane.issue_id) else {
		return Ok(None);
	};
	let issue = issues.swap_remove(issue_index);

	if state_store.issue_has_active_shared_claim(project.service_id(), &issue.id)? {
		return Ok(None);
	}

	Ok(Some(issue))
}

pub(crate) fn select_target_post_review_closeout_issue_candidate_with_inspector<T, I>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	target_issue_id: &str,
	target_issue_reference: &str,
	review_state_inspector: &I,
) -> Result<Option<SelectedIssueRunCandidate>>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
{
	let completed_state = workflow.frontmatter().tracker().resolved_completed_state();
	let lanes = build_post_review_lane_statuses(
		tracker,
		project,
		workflow,
		state_store,
		review_state_inspector,
	)?;
	let closeout_lanes = lanes
		.into_iter()
		.filter(|lane| post_review_lane_is_closeout_candidate(lane, completed_state))
		.collect::<Vec<_>>();

	if closeout_lanes.is_empty() {
		return Ok(None);
	}

	let Some(target_lane) = closeout_lanes.iter().find(|lane| lane.issue_id == target_issue_id)
	else {
		let visible_lanes = closeout_lanes
			.iter()
			.map(|lane| lane.issue_identifier.as_str())
			.collect::<Vec<_>>()
			.join(", ");

		eyre::bail!(
			"targeted retained closeout mismatch: requested issue `{}` does not match status-visible retained closeout lane(s) `{}`",
			target_issue_reference,
			visible_lanes,
		);
	};
	let issue_ids = [target_lane.issue_id.clone()];
	let mut issues = tracker.refresh_issues(&issue_ids)?;
	let Some(issue_index) = issues.iter().position(|issue| issue.id == target_lane.issue_id) else {
		return Ok(None);
	};
	let issue = issues.swap_remove(issue_index);

	if closeout_lane_active_claim_blocks_dispatch(project, state_store, &issue)? {
		return Ok(None);
	}

	let preferred_run_identity =
		retained_closeout_preferred_run_identity(state_store, project.service_id(), &issue)?;

	Ok(Some(SelectedIssueRunCandidate {
		issue,
		dispatch_mode: IssueDispatchMode::Closeout,
		preferred_run_identity,
	}))
}
