use crate::{
	orchestrator::{
		self, IssueDispatchMode, IssueTracker, PullRequestReviewStateInspector,
		SelectedIssueRunCandidate, ServiceConfig, StateStore, TrackerIssue, WorkflowDocument,
		run_cycle_post_review::{closeout_identity, predicates},
	},
	prelude::{Result, eyre},
};

pub(crate) fn select_target_review_repair_candidate_with_inspector<T, I>(
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
	let lanes = orchestrator::build_post_review_lane_statuses(
		tracker,
		project,
		workflow,
		state_store,
		review_state_inspector,
	)?;
	let repair_lanes = lanes
		.into_iter()
		.filter(predicates::post_review_lane_is_repair_candidate)
		.collect::<Vec<_>>();

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

pub(crate) fn select_target_closeout_candidate_with_inspector<T, I>(
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
	let lanes = orchestrator::build_post_review_lane_statuses(
		tracker,
		project,
		workflow,
		state_store,
		review_state_inspector,
	)?;
	let closeout_lanes = lanes
		.into_iter()
		.filter(|lane| predicates::post_review_lane_is_closeout_candidate(lane, completed_state))
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

	if orchestrator::closeout_lane_active_claim_blocks_dispatch(project, state_store, &issue)? {
		return Ok(None);
	}

	let preferred_run_identity = closeout_identity::retained_closeout_preferred_run_identity(
		state_store,
		project.service_id(),
		&issue,
	)?;

	Ok(Some(SelectedIssueRunCandidate {
		issue,
		dispatch_mode: IssueDispatchMode::Closeout,
		preferred_run_identity,
		program_dispatch: None,
	}))
}
