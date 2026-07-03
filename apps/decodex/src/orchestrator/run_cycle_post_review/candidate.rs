use std::{collections::HashMap, path::Path};

use crate::{
	orchestrator::{
		self, GhPullRequestReviewStateInspector, IssueDispatchMode, IssueTracker,
		PullRequestReviewStateInspector, SelectedIssueRunCandidate, ServiceConfig, StateStore,
		TrackerIssue, WorkflowDocument,
		run_cycle_post_review::{closeout_identity, predicates},
	},
	prelude::Result,
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
	let lanes = orchestrator::build_post_review_lane_statuses(
		tracker,
		project,
		workflow,
		state_store,
		review_state_inspector,
	)?;
	let candidate_issue_ids = lanes
		.iter()
		.filter(|lane| predicates::post_review_lane_is_repair_candidate(lane))
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
		if !predicates::post_review_lane_is_repair_candidate(&lane) {
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
	let lanes = orchestrator::build_post_review_lane_statuses(
		tracker,
		project,
		workflow,
		state_store,
		review_state_inspector,
	)?;
	let candidate_issue_ids = lanes
		.iter()
		.filter(|lane| predicates::post_review_lane_is_closeout_candidate(lane, completed_state))
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
		let is_closeout_candidate =
			predicates::post_review_lane_is_closeout_candidate(&lane, completed_state);

		if !is_closeout_candidate {
			continue;
		}
		if excluded_issue_ids.contains(&lane.issue_id.as_str()) {
			continue;
		}

		if let Some(issue) = issues_by_id.remove(&lane.issue_id) {
			if orchestrator::closeout_lane_active_claim_blocks_dispatch(
				project,
				state_store,
				&issue,
			)? {
				continue;
			}

			let preferred_run_identity =
				closeout_identity::retained_closeout_preferred_run_identity(
					state_store,
					project.service_id(),
					&issue,
				)?;

			return Ok(Some(SelectedIssueRunCandidate {
				issue,
				dispatch_mode: IssueDispatchMode::Closeout,
				preferred_run_identity,
				program_dispatch: None,
			}));
		}
	}

	Ok(None)
}
